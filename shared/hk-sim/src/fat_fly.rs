//! Source-derived Fat Fly (Obble, Greenpath): two FSMs on one object, both enabled
//! by its `FSMActivator`. `fat fly bounce` wakes when the Knight is within 25 units,
//! aims at him and flies at 4 units a second, turning off whatever it touches by the
//! contact normal; `Fatty Fly Attack`, started by the first aim, slows it every two to
//! three seconds (or at once on `TAKE DAMAGE`), plays `Attack` and throws four
//! `Spitter Shot R` on the diagonals at 12. Evidence: FSM dumps of level147 and
//! level158 (one digest per FSM for all nine placements), the placement's tk2d
//! library, IL of `Decelerate` (docs/FAT_FLY.md).
//!
//! The caller owns the gravity-free body, the side it was stopped on after its
//! solver step, the projectile pool, Recoil, hits, corpse and Geo. `tick` is one
//! nominal 60 Hz frame; `SetVelocityAsAngle` and `Decelerate` run on the 50 Hz fixed
//! step from an accumulator (PlayMakerFixedUpdate targets both FSMs, the bounce FSM
//! first), `Decelerate` also once on entry.
use crate::ONE;

/// `fat fly bounce`'s `Speed`.
pub const SPEED: i32 = 4 * ONE;
/// `Initialise`'s `GetDistance(Self, Hero) < 25`, every frame.
pub const WAKE_DISTANCE: i32 = 25 * ONE;
/// `Decelerate` 0.1 per fixed step, each axis toward zero.
pub const DECELERATION: i32 = 6554;
/// `Attack`'s `FlingObjectsFromGlobalPool` speed (`Shot Speed` 12) at 45, 135, 225
/// and 315 degrees: 12 cos 45 on each axis.
pub const SHOT_DIAGONAL: i32 = 556_091;
/// `Attack Antic` Wait 0.35 s.
pub const ANTIC_TICKS: u16 = 21;
/// `Attack` (eight frames at 12 fps, once): its trigger frame 5 ends `Attack Antic 2`.
pub const ATTACK_TRIGGER_TICKS: u16 = 25;
/// Its completion, 15 ticks after the trigger, ends `Attack` before its own Wait 0.5 s.
pub const ATTACK_TICKS: u16 = 40;
/// `CD` Wait 0.5 s.
pub const COOLDOWN_TICKS: u16 = 30;
/// `Wait`'s `WaitRandom` 2 to 3 s.
pub const WAIT_TICKS: [u16; 2] = [120, 180];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flight {
    /// `Initialise`: watching the Knight's distance.
    Waiting,
    /// `Fly 2`.
    Flying,
    /// `Stopped`: no velocity is written; what is left of it drifts until `WAKE`.
    Stopped,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    Sleep,
    Wait,
    Antic,
    Antic2,
    Shoot,
    Cooldown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Fly,
    Attack,
}
/// The side a contact stopped the body on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Up,
    Right,
    Down,
    Left,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// The clip restarted this tick.
    pub play: Option<Clip>,
    /// `Attack`: the four diagonal shots leave from the body this tick.
    pub volley: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FatFly {
    flight: Flight,
    attack: Attack,
    /// Q16 degrees counter-clockwise from +x, the FSM's `Angle`.
    angle: i32,
    facing_right: bool,
    velocity: [i32; 2],
    timer: u16,
    fixed_accumulator: u8,
    rng: u32,
}
/// `Mathf.Atan2(y, x)` in degrees 0 to 360, Q16 (4096 table units a turn).
fn degrees(y: i32, x: i32) -> i32 {
    psx_math::sincos::atan2_q12(y, x) as i32 * 5760
}
impl FatFly {
    /// `facing_right`: the placement's authored scale x is -1 (the art faces left).
    pub fn new(facing_right: bool, seed: u32) -> Self {
        Self {
            flight: Flight::Waiting,
            attack: Attack::Sleep,
            angle: 0,
            facing_right,
            velocity: [0; 2],
            timer: 0,
            fixed_accumulator: 0,
            rng: seed,
        }
    }
    pub fn flight(&self) -> Flight {
        self.flight
    }
    pub fn attack(&self) -> Attack {
        self.attack
    }
    pub fn angle(&self) -> i32 {
        self.angle
    }
    pub fn velocity(&self) -> [i32; 2] {
        self.velocity
    }
    /// The draw convention (-1 native): `Face Left` sets scale 1 (the art faces left),
    /// `Face Right` scale -1.
    pub fn facing(&self) -> i32 {
        if self.facing_right {
            1
        } else {
            -1
        }
    }
    pub fn clip(&self) -> Clip {
        if matches!(self.attack, Attack::Antic2 | Attack::Shoot) {
            Clip::Attack
        } else {
            Clip::Fly
        }
    }
    pub fn dead(&self) -> bool {
        self.flight == Flight::Dead
    }
    /// Ticks until `tick` returns the next volley (1: the next tick), `None` while asleep or
    /// dead. In `Wait` it is exact (the random wait is drawn on entry); after a volley it counts
    /// the rest of `Attack` and `CD` plus the shortest wait, a lower bound. Read-only, for the
    /// walker's words.
    pub fn ticks_to_volley(&self) -> Option<u16> {
        const AFTER_WAIT: u16 = ANTIC_TICKS + ATTACK_TRIGGER_TICKS;
        let rest = match (self.flight, self.attack) {
            (Flight::Dead, _) | (_, Attack::Sleep) => return None,
            (_, Attack::Wait) => self.timer + AFTER_WAIT,
            (_, Attack::Antic) => self.timer + ATTACK_TRIGGER_TICKS,
            (_, Attack::Antic2) => self.timer,
            (_, Attack::Shoot) => self.timer + COOLDOWN_TICKS + WAIT_TICKS[0] + AFTER_WAIT,
            (_, Attack::Cooldown) => self.timer + WAIT_TICKS[0] + AFTER_WAIT,
        };
        Some(rest)
    }
    pub fn die(&mut self) {
        self.flight = Flight::Dead;
        self.velocity = [0; 2];
    }
    fn random(&mut self) -> u32 {
        self.rng = psx_fx::NrLcg::step(self.rng);
        self.rng >> 8
    }
    /// `Random.Range(low, high)` over whole Q16 degrees or ticks, `high` included.
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + psx_math::WideU::mul(self.random(), (high - low) as u32 + 1).shr_u32(24) as i32
    }
    /// `Left or Right?`: FloatSwitch below 90 RIGHT, below 270 LEFT, below 360 RIGHT.
    fn face_angle(&mut self) {
        let a = self.angle.rem_euclid(360 * ONE);
        self.facing_right = !(90 * ONE..270 * ONE).contains(&a);
    }
    fn aim_range(&mut self, low: i32, high: i32) {
        self.angle = self.range(low * ONE, high * ONE);
        self.face_angle();
    }
    /// `SetVelocityAsAngle`: the speed along the angle.
    fn write_velocity(&mut self) {
        let q12 = psx_math::int32::mul_div_i32(self.angle.rem_euclid(360 * ONE), 4096, 360 * ONE) as u16;
        self.velocity = [
            psx_math::int32::mul_shr_i32(SPEED, psx_math::sincos::cos_q12(q12), 12),
            psx_math::int32::mul_shr_i32(SPEED, psx_math::sincos::sin_q12(q12), 12),
        ];
    }
    fn decelerate(&mut self) {
        for v in &mut self.velocity {
            *v = if *v > 0 { (*v - DECELERATION).max(0) } else { (*v + DECELERATION).min(0) };
        }
    }
    /// `Wait`: WAKE to the bounce FSM (only `Stopped` answers it, for `Left or Right?`
    /// along the angle it stopped at), then `WaitRandom` 2 to 3 s.
    fn enter_wait(&mut self) {
        self.attack = Attack::Wait;
        self.timer = self.range(WAIT_TICKS[0] as i32, WAIT_TICKS[1] as i32) as u16;
        if self.flight == Flight::Stopped {
            self.flight = Flight::Flying;
            self.face_angle();
        }
    }
    fn enter_antic(&mut self) {
        self.attack = Attack::Antic;
        self.timer = ANTIC_TICKS;
        self.decelerate();
    }
    /// `TAKE DAMAGE`, a global transition of `Fatty Fly Attack`: from wherever it is,
    /// even asleep, it winds up to attack.
    pub fn took_damage(&mut self) {
        if self.flight != Flight::Dead {
            self.enter_antic();
        }
    }
    /// `COLLISION STAY 2D` while flying: `Collision Check` reads the contact normal and
    /// re-aims from the authored range for the facing (a floor or ceiling) or for the
    /// vertical half it was heading in (a wall).
    pub fn bonk(&mut self, side: Side) {
        if self.flight != Flight::Flying {
            return;
        }
        let up = self.angle.rem_euclid(360 * ONE) < 180 * ONE;
        let (low, high) = match (side, self.facing_right, up) {
            (Side::Up, true, _) => (320, 350),
            (Side::Up, false, _) => (190, 220),
            (Side::Down, true, _) => (10, 40),
            (Side::Down, false, _) => (140, 170),
            (Side::Right, _, true) => (140, 170),
            (Side::Right, _, false) => (190, 220),
            (Side::Left, _, true) => (10, 40),
            (Side::Left, _, false) => (320, 350),
        };
        self.aim_range(low, high);
    }
    /// One nominal 60 Hz frame. `hero`: the Knight's offset from the body.
    pub fn tick(&mut self, hero: [i32; 2]) -> Step {
        let mut step = Step::default();
        if self.flight == Flight::Dead {
            return step;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        let decelerating = matches!(self.attack, Attack::Antic | Attack::Antic2);
        match self.flight {
            Flight::Waiting => {
                if hero[0].abs() < WAKE_DISTANCE
                    && hero[1].abs() < WAKE_DISTANCE
                    && crate::fixed::sum_sq(hero[0], hero[1]) < crate::fixed::square(WAKE_DISTANCE)
                {
                    // `Aim`: START to the attack FSM, then the angle to the Knight.
                    if self.attack == Attack::Sleep {
                        self.enter_wait();
                    }
                    self.angle = degrees(hero[1], hero[0]);
                    self.face_angle();
                    self.flight = Flight::Flying;
                }
            }
            Flight::Flying => {
                if fixed {
                    self.write_velocity();
                    if decelerating {
                        self.decelerate();
                    }
                }
            }
            Flight::Stopped => {
                if fixed && decelerating {
                    self.decelerate();
                }
            }
            Flight::Dead => {}
        }
        match self.attack {
            Attack::Sleep => {}
            Attack::Wait => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.enter_antic();
                }
            }
            Attack::Antic => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    // `Attack Antic 2`: STOP to the bounce FSM, whose `Stopped` stores the
                    // angle of the velocity it is left with.
                    self.attack = Attack::Antic2;
                    self.timer = ATTACK_TRIGGER_TICKS;
                    if self.velocity != [0; 2] {
                        self.angle = degrees(self.velocity[1], self.velocity[0]);
                    }
                    self.flight = Flight::Stopped;
                    self.decelerate();
                    step.play = Some(Clip::Attack);
                }
            }
            Attack::Antic2 => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.attack = Attack::Shoot;
                    self.timer = ATTACK_TICKS - ATTACK_TRIGGER_TICKS;
                    step.volley = true;
                }
            }
            Attack::Shoot => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.attack = Attack::Cooldown;
                    self.timer = COOLDOWN_TICKS;
                    step.play = Some(Clip::Fly);
                }
            }
            Attack::Cooldown => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.enter_wait();
                }
            }
        }
        step
    }
    /// The body's velocity after the caller's solver step (a blocked axis is zero).
    pub fn moved(&mut self, velocity: [i32; 2]) {
        self.velocity = velocity;
    }
}
/// The four shots of a volley, in `Attack`'s order (45, 135, 225, 315 degrees).
pub const VOLLEY: [[i32; 2]; 4] = [
    [SHOT_DIAGONAL, SHOT_DIAGONAL],
    [-SHOT_DIAGONAL, SHOT_DIAGONAL],
    [-SHOT_DIAGONAL, -SHOT_DIAGONAL],
    [SHOT_DIAGONAL, -SHOT_DIAGONAL],
];

#[cfg(test)]
mod tests {
    use super::*;

    fn speed(v: [i32; 2]) -> i32 {
        crate::fixed::hypot(v[0], v[1]) as i32
    }
    #[test]
    fn waits_for_the_knight_within_25_then_flies_at_him_at_four() {
        let mut f = FatFly::new(false, 3);
        for _ in 0..100 {
            f.tick([26 * ONE, 0]);
        }
        assert_eq!((f.flight(), f.attack()), (Flight::Waiting, Attack::Sleep));
        f.tick([10 * ONE, 0]);
        assert_eq!((f.flight(), f.attack()), (Flight::Flying, Attack::Wait));
        assert_eq!(f.angle(), 0, "aimed straight at a Knight to its right");
        assert_eq!(f.facing(), 1);
        for _ in 0..3 {
            f.tick([10 * ONE, 0]);
        }
        let v = f.velocity();
        assert!((speed(v) - SPEED).abs() < ONE / 64 && v[0] > 0, "{v:?}");
    }
    #[test]
    fn every_two_to_three_seconds_it_stops_and_throws_four_diagonals() {
        for seed in 0..8 {
            let mut f = FatFly::new(false, seed);
            f.tick([-5 * ONE, 5 * ONE]);
            let mut t = 0;
            while f.attack() != Attack::Antic {
                f.tick([-5 * ONE, 5 * ONE]);
                t += 1;
                assert!(t <= WAIT_TICKS[1] as u32, "waits at most 3 s");
            }
            assert!(t + 1 >= WAIT_TICKS[0] as u32, "waits at least 2 s: {t}");
            let mut ticks = 0;
            let mut volley = None;
            while f.attack() != Attack::Cooldown {
                let s = f.tick([-5 * ONE, 5 * ONE]);
                ticks += 1;
                if s.volley {
                    volley = Some(ticks);
                }
            }
            assert_eq!(volley, Some(ANTIC_TICKS as u32 + ATTACK_TRIGGER_TICKS as u32));
            assert_eq!(ticks, (ANTIC_TICKS + ATTACK_TICKS) as u32);
            assert_eq!(f.flight(), Flight::Stopped);
            // CD then Wait's WAKE flies on.
            for _ in 0..COOLDOWN_TICKS {
                f.tick([0, 0]);
            }
            assert_eq!((f.flight(), f.attack()), (Flight::Flying, Attack::Wait));
        }
    }
    #[test]
    fn a_hit_winds_it_up_at_once_even_asleep() {
        let mut f = FatFly::new(true, 1);
        f.took_damage();
        assert_eq!(f.attack(), Attack::Antic);
        for _ in 0..ANTIC_TICKS {
            f.tick([40 * ONE, 0]);
        }
        assert_eq!((f.attack(), f.flight()), (Attack::Antic2, Flight::Stopped));
    }
    #[test]
    fn ticks_to_volley_counts_down_to_the_volley_tick() {
        let mut f = FatFly::new(false, 3);
        assert_eq!(f.ticks_to_volley(), None);
        f.tick([ONE, 0]);
        let mut left = f.ticks_to_volley().expect("awake");
        loop {
            let step = f.tick([ONE, 0]);
            if step.volley {
                assert_eq!(left, 1);
                break;
            }
            let now = f.ticks_to_volley().expect("awake");
            assert_eq!(now, left - 1);
            left = now;
        }
        assert!(f.ticks_to_volley().expect("awake") > WAIT_TICKS[0]);
        f.die();
        assert_eq!(f.ticks_to_volley(), None);
    }
    #[test]
    fn a_contact_reaims_from_the_authored_ranges() {
        let mut f = FatFly::new(false, 9);
        f.tick([10 * ONE, 10 * ONE]);
        assert!(f.facing() > 0);
        f.bonk(Side::Up);
        assert!((320 * ONE..=350 * ONE).contains(&f.angle()), "{}", f.angle());
        assert!(f.facing() > 0);
        f.bonk(Side::Right);
        // Heading down (320..350): `Right Down` 190..220, and it now faces left.
        assert!((190 * ONE..=220 * ONE).contains(&f.angle()));
        assert!(f.facing() < 0);
        f.bonk(Side::Down);
        assert!((140 * ONE..=170 * ONE).contains(&f.angle()));
        f.bonk(Side::Left);
        assert!((10 * ONE..=40 * ONE).contains(&f.angle()));
    }
}
