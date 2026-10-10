//! Source-derived Fat Fly: the `fat fly bounce` FSM (a Gruzzer's flight, woken
//! by the Knight coming within 25 units and first aimed at him) and the
//! `Fatty Fly Attack` FSM that, every two to three seconds or at once when it
//! is hit, slows to a halt, plays `Attack` and throws four `Spitter Shot R`
//! diagonally. Evidence: docs/FAT_FLY.md (FSM dump of level147 and level153).
//!
//! The caller owns physics (a gravity-free dynamic body), the blocked side
//! after its solver step (CheckCollisionSide priority up, right, down, left),
//! the projectile pool, Recoil, hits, corpse and Geo. `tick` is one nominal
//! 60 Hz frame; the source 50 Hz FixedUpdate steps (SetVelocityAsAngle and
//! Decelerate) run from an accumulator so their per-step constants stay
//! authored. `Random.Range` is a deterministic per-actor sequence.
use crate::ONE;

/// `fat fly bounce`'s `Speed`.
pub const SPEED: i32 = 4 * ONE;
/// `Initialise`'s `GetDistance(Self, Hero) < 25`.
pub const WAKE_DISTANCE: i32 = 25 * ONE;
/// `Decelerate` 0.1 per fixed step, on each axis toward zero.
pub const DECELERATION: i32 = 6554;
/// `Attack`'s `FlingObjectsFromGlobalPool` speed (`Shot Speed`).
pub const SHOT_SPEED: i32 = 12 * ONE;
/// 12 * cos 45 degrees: the four shots leave on the diagonals.
pub const SHOT_DIAGONAL: i32 = 556_091;
/// `Attack Antic` Wait 0.35 s.
pub const ANTIC_TICKS: u16 = 21;
/// `Attack` frame 5 of 12 at 12 fps: the trigger that ends `Attack Antic 2`.
pub const ATTACK_TRIGGER_TICKS: u16 = 25;
/// Eight frames at 12 fps: `Attack`'s animation-complete event, 15 ticks
/// after the trigger, before the state's own Wait 0.5 s would end it.
pub const ATTACK_TICKS: u16 = 40;
/// `CD` Wait 0.5 s.
pub const COOLDOWN_TICKS: u16 = 30;
/// `Wait`'s `WaitRandom` 2.0 to 3.0 s.
pub const WAIT_TICKS: [u16; 2] = [120, 180];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flight {
    /// `Initialise`: watching the Knight's distance.
    Waiting,
    Flying,
    /// `Stopped`: no velocity is written; what is left of it drifts.
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
impl Clip {
    pub const COUNT: usize = 2;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Up,
    Right,
    Down,
    Left,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Clip),
    /// One `Spitter Shot R` leaves the actor with this Q16 velocity.
    Fire([i32; 2]),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 6],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 6],
            count: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize]
            .iter()
            .map(|a| a.unwrap())
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
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
/// Floor square root, for the tests' speed check.
#[cfg(test)]
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        0
    } else {
        psx_math::int32::isqrt_u64(n as u64) as i64
    }
}
/// `Mathf.Atan2(y, x)` in degrees, 0 to 360, as Q16.
fn degrees(y: i32, x: i32) -> i32 {
    let q12 = psx_math::atan2_q12(y, x) as i64;
    (q12 * 360 * ONE as i64 / 4096) as i32
}
impl FatFly {
    pub fn new(seed: u32) -> Self {
        Self {
            flight: Flight::Waiting,
            attack: Attack::Sleep,
            angle: 0,
            facing_right: false,
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
    /// `Face Left` sets the scale to 1 (the art faces left), `Face Right` to -1.
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
    pub fn die(&mut self) {
        self.flight = Flight::Dead;
        self.velocity = [0; 2];
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    /// `Left or Right?`: FloatSwitch below 90 RIGHT, below 270 LEFT, else RIGHT.
    fn face_angle(&mut self) {
        let a = self.angle.rem_euclid(360 * ONE);
        self.facing_right = !(90 * ONE..270 * ONE).contains(&a);
    }
    fn aim_range(&mut self, low: i32, high: i32) {
        self.angle = self.range(low * ONE, high * ONE);
        self.face_angle();
    }
    /// `SetVelocityAsAngle`: the speed along the angle, written each fixed step.
    fn write_velocity(&mut self) {
        let a = (self.angle.rem_euclid(360 * ONE) as i64 * 4096 / (360 * ONE as i64)) as u16;
        self.velocity = [
            ((SPEED as i64 * psx_math::cos_q12(a) as i64) >> 12) as i32,
            ((SPEED as i64 * psx_math::sin_q12(a) as i64) >> 12) as i32,
        ];
    }
    /// `Decelerate`: each axis toward zero by 0.1, never past it.
    fn decelerate(&mut self) {
        for v in &mut self.velocity {
            *v = if *v > 0 {
                (*v - DECELERATION).max(0)
            } else {
                (*v + DECELERATION).min(0)
            };
        }
    }
    fn enter_wait(&mut self) {
        self.attack = Attack::Wait;
        self.timer = self.range(WAIT_TICKS[0] as i32, WAIT_TICKS[1] as i32 + 1) as u16;
        // `Wait` sends WAKE to `fat fly bounce`: only `Stopped` answers it,
        // leaving for `Left or Right?` along the angle it stopped at.
        if self.flight == Flight::Stopped {
            self.flight = Flight::Flying;
            self.face_angle();
        }
    }
    fn enter_antic(&mut self) {
        self.attack = Attack::Antic;
        self.timer = ANTIC_TICKS;
    }
    /// `TAKE DAMAGE`, a global transition of `Fatty Fly Attack`: from wherever
    /// it is, it winds up to attack.
    pub fn took_damage(&mut self) {
        if self.flight != Flight::Dead {
            self.enter_antic();
        }
    }
    /// CheckCollisionSide events while flying: each side re-aims from the
    /// authored range for the current facing or vertical half.
    pub fn bonk(&mut self, side: Side) {
        if self.flight != Flight::Flying {
            return;
        }
        let up = self.angle.rem_euclid(360 * ONE) < 180 * ONE;
        match (side, self.facing_right, up) {
            (Side::Up, true, _) => self.aim_range(320, 350),
            (Side::Up, false, _) => self.aim_range(190, 220),
            (Side::Down, true, _) => self.aim_range(10, 40),
            (Side::Down, false, _) => self.aim_range(140, 170),
            (Side::Right, _, true) => self.aim_range(140, 170),
            (Side::Right, _, false) => self.aim_range(190, 220),
            (Side::Left, _, true) => self.aim_range(10, 40),
            (Side::Left, _, false) => self.aim_range(320, 350),
        }
    }
    fn volley(out: &mut Actions) {
        for (sx, sy) in [(1, 1), (-1, 1), (-1, -1), (1, -1)] {
            out.push(Action::Fire([sx * SHOT_DIAGONAL, sy * SHOT_DIAGONAL]));
        }
    }
    /// One nominal 60 Hz frame.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.flight == Flight::Dead {
            return out;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        // `fat fly bounce` runs before `Fatty Fly Attack` in the fixed update.
        let decelerating = matches!(self.attack, Attack::Antic | Attack::Antic2);
        match self.flight {
            Flight::Waiting => {
                let dx = senses.hero[0] as i64 - senses.position[0] as i64;
                let dy = senses.hero[1] as i64 - senses.position[1] as i64;
                let limit = WAKE_DISTANCE as i64;
                if dx.abs() < limit && dy.abs() < limit && dx * dx + dy * dy < limit * limit {
                    // `Aim`: START to the attack FSM, then the angle to the Knight.
                    if self.attack == Attack::Sleep {
                        self.enter_wait();
                    }
                    self.angle = degrees(dy as i32, dx as i32);
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
                    // `Attack Antic 2`: STOP to the bounce FSM, which keeps the
                    // angle of the velocity it was left with.
                    self.attack = Attack::Antic2;
                    self.timer = ATTACK_TRIGGER_TICKS;
                    if self.velocity != [0; 2] {
                        self.angle = degrees(self.velocity[1], self.velocity[0]);
                    }
                    self.flight = Flight::Stopped;
                    out.push(Action::Play(Clip::Attack));
                }
            }
            Attack::Antic2 => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.attack = Attack::Shoot;
                    self.timer = ATTACK_TICKS - ATTACK_TRIGGER_TICKS;
                    Self::volley(&mut out);
                }
            }
            Attack::Shoot => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.attack = Attack::Cooldown;
                    self.timer = COOLDOWN_TICKS;
                    out.push(Action::Play(Clip::Fly));
                }
            }
            Attack::Cooldown => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.enter_wait();
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEAR: Senses = Senses {
        position: [0, 0],
        hero: [10 * ONE, 0],
    };
    const FAR: Senses = Senses {
        position: [0, 0],
        hero: [30 * ONE, 0],
    };

    fn run(f: &mut FatFly, senses: Senses, ticks: u32) -> u32 {
        let mut shots = 0;
        for _ in 0..ticks {
            shots += f
                .tick(senses)
                .iter()
                .filter(|a| matches!(a, Action::Fire(_)))
                .count() as u32;
        }
        shots
    }

    #[test]
    fn sleeps_until_the_knight_is_within_twenty_five_units() {
        let mut f = FatFly::new(3);
        run(&mut f, FAR, 600);
        assert_eq!((f.flight(), f.attack()), (Flight::Waiting, Attack::Sleep));
        f.tick(Senses {
            hero: [24 * ONE, 0],
            ..FAR
        });
        assert_eq!(f.flight(), Flight::Flying, "24 units is inside");
        assert_eq!(f.attack(), Attack::Wait, "START wakes the attack FSM");
    }

    #[test]
    fn first_aim_is_at_the_knight_and_it_flies_at_four() {
        let mut f = FatFly::new(3);
        f.tick(Senses {
            position: [0, 0],
            hero: [-10 * ONE, 0],
        });
        assert_eq!(f.facing(), -1, "the Knight is to the left");
        f.tick(NEAR);
        f.tick(NEAR);
        let [vx, vy] = f.velocity();
        assert!(vx < -3 * ONE && vy.abs() < ONE / 8, "{vx} {vy}");
        let mut g = FatFly::new(3);
        g.tick(NEAR);
        assert_eq!(g.facing(), 1);
        for _ in 0..3 {
            g.tick(NEAR);
        }
        let speed = isqrt((g.velocity()[0] as i64).pow(2) + (g.velocity()[1] as i64).pow(2));
        assert!(
            (SPEED as i64 - 200..=SPEED as i64 + 200).contains(&speed),
            "{speed}"
        );
    }

    #[test]
    fn bonks_re_aim_from_the_authored_ranges() {
        let deg = |a: i32| a / ONE;
        for seed in 0..64u32 {
            let mut f = FatFly::new(seed);
            f.tick(NEAR);
            f.bonk(Side::Down);
            let right = f.facing() == 1;
            let a = deg(f.angle());
            assert!(
                if right {
                    (10..=40).contains(&a)
                } else {
                    (140..=170).contains(&a)
                },
                "{a} {right}"
            );
            f.bonk(Side::Up);
            let a = deg(f.angle());
            assert!(
                if right {
                    (320..=350).contains(&a)
                } else {
                    (190..=220).contains(&a)
                },
                "{a} {right}"
            );
            f.bonk(Side::Right);
            assert!((190..=220).contains(&deg(f.angle())));
            f.bonk(Side::Left);
            assert!((320..=350).contains(&deg(f.angle())));
        }
    }

    #[test]
    fn two_to_three_seconds_after_waking_it_throws_four_shots_on_the_diagonals() {
        let mut f = FatFly::new(7);
        f.tick(NEAR);
        let mut volley = None;
        for t in 0..600u32 {
            let mut shots = [None; 4];
            let mut n = 0;
            for a in f.tick(NEAR).iter() {
                if let Action::Fire(v) = a {
                    shots[n] = Some(v);
                    n += 1;
                }
            }
            if n != 0 {
                volley = Some((t, n, shots));
                break;
            }
        }
        let (t, n, shots) = volley.expect("it must attack");
        assert_eq!(n, 4);
        // Wait 2 to 3 s, Antic 0.35 s, the Attack clip's frame 5 at 12 fps.
        let low = WAIT_TICKS[0] as u32 + ANTIC_TICKS as u32 + ATTACK_TRIGGER_TICKS as u32;
        let high = WAIT_TICKS[1] as u32 + ANTIC_TICKS as u32 + ATTACK_TRIGGER_TICKS as u32;
        assert!((low - 2..=high + 2).contains(&t), "{t}");
        let mut signs: [(i32, i32); 4] = [(0, 0); 4];
        for (i, s) in shots.iter().enumerate() {
            let [vx, vy] = s.unwrap();
            assert_eq!(vx.abs(), SHOT_DIAGONAL);
            assert_eq!(vy.abs(), SHOT_DIAGONAL);
            signs[i] = (vx.signum(), vy.signum());
        }
        signs.sort();
        assert_eq!(signs, [(-1, -1), (-1, 1), (1, -1), (1, 1)]);
        assert_eq!(f.clip(), Clip::Attack);
    }

    #[test]
    fn a_hit_winds_it_up_at_once_and_it_slows_to_the_drift_it_keeps() {
        let mut f = FatFly::new(9);
        f.tick(NEAR);
        run(&mut f, NEAR, 30);
        f.took_damage();
        assert_eq!(f.attack(), Attack::Antic);
        let before = f.velocity();
        run(&mut f, NEAR, ANTIC_TICKS as u32);
        assert_eq!(f.attack(), Attack::Antic2);
        assert_eq!(f.flight(), Flight::Stopped);
        // Decelerating on both axes since the hit, and never past zero.
        let after = f.velocity();
        assert!(after[0].abs() <= before[0].abs() && after[1].abs() <= before[1].abs());
        let shots = run(&mut f, NEAR, ATTACK_TRIGGER_TICKS as u32);
        assert_eq!(shots, 4);
        let drift = f.velocity();
        run(&mut f, NEAR, 5);
        assert_eq!(
            f.velocity(),
            drift,
            "Stopped writes no velocity and Attack does not decelerate"
        );
    }

    #[test]
    fn after_the_cooldown_it_flies_on_and_attacks_again() {
        let mut f = FatFly::new(11);
        f.tick(NEAR);
        let shots = run(&mut f, NEAR, 2400);
        assert!(
            shots >= 8,
            "four per attack, several attacks in 40 s: {shots}"
        );
        assert_eq!(f.flight(), Flight::Flying);
    }

    #[test]
    fn dead_is_inert() {
        let mut f = FatFly::new(1);
        f.tick(NEAR);
        f.die();
        assert!(f.tick(NEAR).is_empty());
        f.took_damage();
        f.bonk(Side::Up);
        assert!(f.dead());
        assert_eq!(f.velocity(), [0; 2]);
    }
}
