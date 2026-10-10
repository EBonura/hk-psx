//! Source-derived Husk Guard (`Zombie Guard` FSM) controller, Crossroads_21 and _48.
//!
//! Motion is Q16.16 units/second and one `tick` is 1/60 s. The caller owns the
//! body (gravity, terrain), health, the sense boxes and line of sight, the
//! `Swipe` hitbox, the pooled `Slam Effect R` and `Shockwave Wave`s this asks
//! for, and the animation clock; a Once clip's completion comes back through
//! `Senses::completed` with the token handed out, as `crate::zombie_shield`.
//!
//! Every number is the FSM's own, proven by host/husk_guard.py before a
//! placement is admitted. The states follow the FSM one for one, with the
//! decision states (`Alert`, `Check Left`, `Chase`, `Face Hero`, `Attack
//! Choice`, ...) resolved in the tick that reaches them, which is where
//! PlayMaker resolves an event sent on state entry.
//!
//! `facing` is the FSM's `Facing Right` (+1) or not (-1). The FSM gives that
//! the transform's +1 scale, while the art, the `Swipe` polygon and the run
//! dust all put the front at -x and `Slam Origin`/`Burst Rocks Club` at +x.
//! The port takes the one reading where the body moves the way it looks: the
//! front is `facing`, the art is drawn mirrored when facing right, and the
//! Swipe box (cooked in the art's frame) is mirrored with it.
//!
//! `Stomp Cooldown` waits 0.4 s with `WAIT` but only has a `FINISHED`
//! transition, so read literally the FSM would leave the guard standing after
//! its first stomp. The shipped game's guards keep fighting, so the port takes
//! the evident intent: after the wait it goes on to `Cooldown` and `Idle`.
use crate::ONE;

/// `Alert Range New`, `Attack Range` (after `Wake`'s SetScale x 10, and as authored: `DORMANT_ATTACK`),
/// `Overhead Detect` and the `Swipe` polygon's bounds: Q16, relative to the
/// actor origin, in the art frame (front at -x).
pub const ALERT: [i32; 4] = [-1102971, -259850, 1102971, 181207];
pub const ATTACK: [i32; 4] = [-327680, -246088, 327680, 172687];
/// `Attack Range` as authored, before `Wake` rescales it: 16.29 wide. `Dormant` answers only its
/// ATTACK ALERT (and damage), so this, not `ALERT`, is how near the hero must come to wake the guard.
pub const DORMANT_ATTACK: [i32; 4] = [-533791, -246088, 533791, 172687];
pub const OVERHEAD: [i32; 4] = [-101253, 12880, 101253, 244406];
pub const SWIPE: [i32; 4] = [-323584, -70656, 142336, 299008];
/// `Swipe`'s DamageHero, armed for its own 3-frame 15 fps clip.
pub const SWIPE_DAMAGE: u16 = 2;
pub const SWIPE_TICKS: u16 = 12;
/// `Check Left`/`Check Right`: Walk Speed and Run Speed.
pub const WALK_SPEED: i32 = 5 * ONE;
pub const RUN_SPEED: i32 = 10 * ONE;
/// `Chase Distance`: farther than this from the hero, it runs.
pub const CHASE_DISTANCE: i32 = 9 * ONE;
/// `Roam Distance` either side of the spawn, and the `Idle Spot` half width.
pub const ROAM_DISTANCE: i32 = 1540096; // 23.5
pub const IDLE_SPOT: i32 = ONE;
/// `Idle`'s Wait 4, `Cooldown`'s 0.21, `Attack`'s 0.201, `Attack Recoil`'s
/// 0.14, `Stomp Antic`'s 0.35 (all in ticks).
pub const IDLE_TICKS: u16 = 240;
pub const COOLDOWN_TICKS: u16 = 13;
pub const ATTACK_TICKS: u16 = 12;
pub const RECOIL_TICKS: u16 = 8;
pub const STOMP_ANTIC_TICKS: u16 = 21;
/// `Stomp Cooldown`'s Wait 0.4 (see the module note on its missing exit).
pub const STOMP_COOLDOWN_TICKS: u16 = 24;
/// `Start Left`/`Turn Left` Recoil 5, away from the front.
pub const RECOIL_SPEED: i32 = 5 * ONE;
/// `Jump L`/`Jump R`: 10 away from the front.
pub const JUMP_SPEED: i32 = 10 * ONE;
/// `Impact Left`/`Impact Right`: `Slam Origin` (4.633 ahead, 3 down).
pub const SLAM_ORIGIN: [i32; 2] = [303628, -3 * ONE];
/// `Land`'s spawn offsets: the slam effect, and the two shockwaves.
pub const STOMP_SLAM_Y: i32 = -247071; // -3.77
pub const WAVE_Y: i32 = -4 * ONE;
/// `Attack Choice`'s SendRandomEvent weights (0.75 club, 0.25 stomp), and
/// the repeat caps of `Club Repeat Check`/`Stomp Repeat Check`.
pub const MAX_CLUBS: u8 = 4;
/// `Land`'s two `Shockwave Wave`s: the False Knight's prefab at `Speed` 18 and
/// x scale 1.25 (host/husk_guard.py reads and proves every field).
pub const WAVE: crate::shockwave::Params = crate::shockwave::Params {
    start_speed: 29491,
    accel: 2359296,
    wave_box: [-46363, 21182, 7464, 115753],
    ground_ray: 104858,
    spurt_box: [-16352, -118, 10151, 114151],
    damage_from: 3,
    damage_to: 6,
    damage: 1,
    spurt_ticks: 18,
};
pub const MAX_STOMPS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Walk,
    Turn,
    Dormant,
    Wake,
    Idle,
    Run,
    StopRun,
    StopWalk,
    Anticipate,
    Attack,
    Startle,
    StompAntic,
    StompJump,
    StompLand,
}
impl Clip {
    /// Clips carried by `ActorController::HuskGuard`; Walk and Turn are the
    /// shared `ActorSpec` slots.
    pub const COUNT: usize = 12;
    pub const fn slot(self) -> Option<usize> {
        match self {
            Self::Walk | Self::Turn => None,
            other => Some(other as usize - 2),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    pub clip: Clip,
    pub serial: u32,
}
/// Where a turn goes when its clip ends: `Turn Left`/`Turn Right` to `Idle`,
/// `Turn Left 2`/`Turn Right 2` on to the walk home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterTurn {
    Idle,
    Return,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Dormant,
    Wake,
    Cooldown(u16),
    Idle(u16),
    Startle,
    Turn(AfterTurn),
    Walk,
    Run,
    StopWalk,
    StopRun,
    Return,
    Anticipate,
    Attack(u16),
    Recoil(u16),
    AttackEnd,
    StompAntic(u16),
    InAir,
    Land,
    /// `Stomp Cooldown`, left after its wait (see the module note).
    StompCooldown(u16),
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Animation),
    /// `Attack`'s ActivateGameObject Swipe: the hitbox lives `SWIPE_TICKS`.
    Swipe,
    /// A pooled `Slam Effect R` at this world point, drawn facing `facing`.
    Slam([i32; 2]),
    /// `Land`: one `Shockwave Wave` each way from this world point.
    Shockwaves([i32; 2]),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 6],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 6],
            len: 0,
        }
    }
    fn push(&mut self, action: Action) {
        if (self.len as usize) < self.commands.len() {
            self.commands[self.len as usize] = Some(action);
            self.len += 1;
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.commands[..self.len as usize]
            .iter()
            .map(|a| a.unwrap())
    }
}
/// Sampled once per tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    pub can_see_hero: bool,
    pub in_alert_range: bool,
    pub in_attack_range: bool,
    /// The hero is inside `DORMANT_ATTACK`, which is the only range a `Dormant` guard listens to.
    pub in_dormant_range: bool,
    /// `Overhead Detect`'s HERO ABOVE, sent while the hero is in its box.
    pub hero_above: bool,
    pub completed: Option<Animation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HuskGuard {
    phase: Phase,
    facing: i8,
    home_x: i32,
    woken: bool,
    running: bool,
    clubs: u8,
    stomps: u8,
    /// Walk Speed and Run Speed as `Check Left`/`Check Right` last set them.
    walk: i32,
    run: i32,
    vx: i32,
    animation: Option<Animation>,
    serial: u32,
    rng: u32,
}
impl HuskGuard {
    /// `Initiate`: Dormant, facing as `Start Facing Left` says, roam and idle
    /// spots around the spawn.
    pub fn new(home_x: i32, facing: i8, seed: u32) -> Self {
        let mut guard = Self {
            phase: Phase::Dormant,
            facing,
            home_x,
            woken: true,
            running: false,
            clubs: 0,
            stomps: 0,
            walk: 0,
            run: 0,
            vx: 0,
            animation: None,
            serial: 0,
            rng: seed | 1,
        };
        let mut actions = Actions::new();
        guard.play(Clip::Dormant, &mut actions);
        guard
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn facing(&self) -> i32 {
        self.facing as i32
    }
    pub fn velocity_x(&self) -> i32 {
        self.vx
    }
    pub fn animation(&self) -> Option<Animation> {
        self.animation
    }
    pub fn clip(&self) -> Clip {
        self.animation.map_or(Clip::Dormant, |a| a.clip)
    }
    /// The `Swipe` box in world space for a body at `position`.
    pub fn swipe_box(&self, position: [i32; 2]) -> [i32; 4] {
        facing_box(SWIPE, self.facing as i32, position)
    }
    fn play(&mut self, clip: Clip, actions: &mut Actions) {
        // tk2dSpriteAnimator.Play of the clip already playing continues it.
        if self.animation.is_some_and(|a| a.clip == clip) {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        let animation = Animation {
            clip,
            serial: self.serial,
        };
        self.animation = Some(animation);
        actions.push(Action::Play(animation));
    }
    fn replay(&mut self, clip: Clip, actions: &mut Actions) {
        self.animation = None;
        self.play(clip, actions);
    }
    fn done(&self, senses: &Senses) -> bool {
        senses.completed.is_some() && senses.completed == self.animation
    }
    fn roaming(&self, hero_x: i32) -> bool {
        (self.home_x - ROAM_DISTANCE..=self.home_x + ROAM_DISTANCE).contains(&hero_x)
    }
    /// `HealthManager`'s TAKE DAMAGE/TOOK DAMAGE, which only `Dormant` answers.
    pub fn took_damage(&mut self) -> Actions {
        let mut actions = Actions::new();
        if self.phase == Phase::Dormant {
            self.enter(Phase::Wake, &mut actions);
        }
        actions
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.vx = 0;
    }
    fn enter(&mut self, phase: Phase, actions: &mut Actions) {
        self.phase = phase;
        match phase {
            Phase::Wake => self.replay(Clip::Wake, actions),
            Phase::Cooldown(_) | Phase::StompCooldown(_) => self.play(Clip::Idle, actions),
            Phase::Idle(_) => {
                self.running = false;
                self.vx = 0;
                self.play(Clip::Idle, actions);
            }
            Phase::Startle => {
                self.replay(Clip::Startle, actions);
                self.woken = true;
            }
            Phase::Turn(_) => self.replay(Clip::Turn, actions),
            Phase::Walk => {
                self.vx = self.walk;
                self.play(Clip::Walk, actions);
            }
            Phase::Run => {
                self.running = true;
                self.vx = self.run;
                self.play(Clip::Run, actions);
            }
            Phase::StopWalk => {
                self.vx = 0;
                self.replay(Clip::StopWalk, actions);
            }
            Phase::StopRun => {
                self.vx = 0;
                self.running = false;
                self.replay(Clip::StopRun, actions);
            }
            Phase::Return => {
                self.woken = false;
                self.running = false;
                self.vx = self.facing as i32 * WALK_SPEED;
                self.play(Clip::Walk, actions);
            }
            Phase::Anticipate => {
                self.running = false;
                self.vx = 0;
                self.replay(Clip::Anticipate, actions);
            }
            Phase::Attack(_) => {
                actions.push(Action::Swipe);
                self.replay(Clip::Attack, actions);
            }
            Phase::Recoil(_) => {
                self.vx = -(self.facing as i32) * RECOIL_SPEED;
            }
            Phase::AttackEnd => self.vx = 0,
            Phase::StompAntic(_) => {
                self.vx = 0;
                self.replay(Clip::StompAntic, actions);
            }
            Phase::InAir => {
                self.vx = -(self.facing as i32) * JUMP_SPEED;
                self.replay(Clip::StompJump, actions);
            }
            Phase::Land => {
                self.vx = 0;
                self.replay(Clip::StompLand, actions);
            }
            Phase::Dormant | Phase::Dead => {}
        }
    }
    /// `Alert` and the chain it starts: turn if the hero is behind, else chase.
    fn alert(&mut self, senses: &Senses, actions: &mut Actions) {
        let (hero, me) = (senses.hero[0], senses.position[0]);
        if hero == me {
            return;
        }
        let dir: i8 = if hero < me { -1 } else { 1 };
        self.walk = dir as i32 * WALK_SPEED;
        self.run = dir as i32 * RUN_SPEED;
        if self.facing != dir {
            self.facing = dir;
            self.enter(Phase::Turn(AfterTurn::Idle), actions);
        } else if self.running || distance(senses.position, senses.hero) > CHASE_DISTANCE {
            self.enter(Phase::Run, actions);
        } else {
            self.enter(Phase::Walk, actions);
        }
    }
    /// `Face Hero`, `Check Left 2`/`Check Right 2` and `Attack Choice`.
    fn face_hero(&mut self, senses: &Senses, actions: &mut Actions) {
        let (hero, me) = (senses.hero[0], senses.position[0]);
        if hero == me {
            return;
        }
        let dir: i8 = if hero < me { -1 } else { 1 };
        self.walk = dir as i32 * WALK_SPEED;
        self.run = dir as i32 * RUN_SPEED;
        if self.facing != dir {
            self.facing = dir;
            self.enter(Phase::Turn(AfterTurn::Idle), actions);
            return;
        }
        loop {
            self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let club = !(self.rng >> 8).is_multiple_of(4);
            if club && self.clubs < MAX_CLUBS {
                self.clubs += 1;
                self.stomps = 0;
                self.enter(Phase::Anticipate, actions);
                return;
            }
            if !club && self.stomps < MAX_STOMPS {
                self.stomps += 1;
                self.clubs = 0;
                self.enter(Phase::StompAntic(0), actions);
                return;
            }
        }
    }
    /// `Walk`/`Run`'s every-frame checks, then back through `Alert`.
    fn pursue(&mut self, senses: &Senses, walking: bool, actions: &mut Actions) {
        if walking && distance(senses.position, senses.hero) > CHASE_DISTANCE {
            self.enter(Phase::Run, actions);
            return;
        }
        if senses.can_see_hero && senses.in_attack_range {
            self.face_hero(senses, actions);
            return;
        }
        if !senses.can_see_hero || !senses.in_alert_range || !self.roaming(senses.hero[0]) {
            self.enter(
                if walking {
                    Phase::StopWalk
                } else {
                    Phase::StopRun
                },
                actions,
            );
            return;
        }
        self.alert(senses, actions);
    }
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut actions = Actions::new();
        let see = senses.can_see_hero;
        match self.phase {
            Phase::Dormant => {
                if senses.in_dormant_range && see {
                    self.enter(Phase::Wake, &mut actions);
                }
            }
            Phase::Wake => {
                if self.done(&senses) {
                    self.enter(Phase::Cooldown(0), &mut actions);
                }
            }
            Phase::Cooldown(t) => {
                if t + 1 >= COOLDOWN_TICKS {
                    self.enter(Phase::Idle(0), &mut actions);
                } else {
                    self.phase = Phase::Cooldown(t + 1);
                }
            }
            Phase::Idle(t) => {
                if senses.in_attack_range && see {
                    self.face_hero(&senses, &mut actions);
                } else if senses.in_alert_range && see && self.roaming(senses.hero[0]) {
                    // `In Roam Distance?` asks the same range again, then `Woken?`.
                    if self.woken {
                        self.alert(&senses, &mut actions);
                    } else {
                        self.enter(Phase::Startle, &mut actions);
                    }
                } else if t + 1 >= IDLE_TICKS {
                    // `Return Check`.
                    let x = senses.position[0];
                    let dir: i8 = if x > self.home_x + IDLE_SPOT {
                        -1
                    } else if x < self.home_x - IDLE_SPOT {
                        1
                    } else {
                        0
                    };
                    if dir == 0 {
                        self.phase = Phase::Idle(0);
                    } else if self.facing != dir {
                        self.facing = dir;
                        self.enter(Phase::Turn(AfterTurn::Return), &mut actions);
                    } else {
                        self.enter(Phase::Return, &mut actions);
                    }
                } else {
                    self.phase = Phase::Idle(t + 1);
                }
            }
            Phase::Startle => {
                if self.done(&senses) {
                    self.alert(&senses, &mut actions);
                }
            }
            Phase::Turn(then) => {
                if senses.hero_above {
                    self.enter(Phase::Anticipate, &mut actions);
                } else if self.done(&senses) {
                    match then {
                        AfterTurn::Idle => self.enter(Phase::Idle(0), &mut actions),
                        AfterTurn::Return => self.enter(Phase::Return, &mut actions),
                    }
                }
            }
            Phase::Walk => self.pursue(&senses, true, &mut actions),
            Phase::Run => self.pursue(&senses, false, &mut actions),
            Phase::StopWalk | Phase::StopRun => {
                if self.done(&senses) {
                    self.enter(Phase::Idle(0), &mut actions);
                }
            }
            Phase::Return => {
                let x = senses.position[0];
                if senses.in_attack_range && see {
                    self.face_hero(&senses, &mut actions);
                } else if senses.in_alert_range && see && self.roaming(senses.hero[0]) {
                    self.alert(&senses, &mut actions);
                } else if (self.facing > 0 && x > self.home_x - IDLE_SPOT)
                    || (self.facing < 0 && x < self.home_x + IDLE_SPOT)
                {
                    self.enter(Phase::Idle(0), &mut actions);
                }
            }
            Phase::Anticipate => {
                if self.done(&senses) {
                    self.enter(Phase::Attack(0), &mut actions);
                }
            }
            Phase::Attack(t) => {
                if t + 1 >= ATTACK_TICKS {
                    let f = self.facing as i32;
                    actions.push(Action::Slam([
                        senses.position[0] + f * SLAM_ORIGIN[0],
                        senses.position[1] + SLAM_ORIGIN[1],
                    ]));
                    self.enter(Phase::Recoil(0), &mut actions);
                } else {
                    self.phase = Phase::Attack(t + 1);
                }
            }
            Phase::Recoil(t) => {
                if t + 1 >= RECOIL_TICKS {
                    self.enter(Phase::AttackEnd, &mut actions);
                } else {
                    self.phase = Phase::Recoil(t + 1);
                }
            }
            Phase::AttackEnd => {
                if self.done(&senses) {
                    self.enter(Phase::Cooldown(0), &mut actions);
                }
            }
            Phase::StompAntic(t) => {
                if t + 1 >= STOMP_ANTIC_TICKS {
                    self.enter(Phase::InAir, &mut actions);
                } else {
                    self.phase = Phase::StompAntic(t + 1);
                }
            }
            Phase::InAir => {
                if self.done(&senses) {
                    self.enter(Phase::Land, &mut actions);
                    let p = senses.position;
                    actions.push(Action::Slam([p[0], p[1] + STOMP_SLAM_Y]));
                    actions.push(Action::Shockwaves([p[0], p[1] + WAVE_Y]));
                }
            }
            Phase::Land => {
                if self.done(&senses) {
                    self.enter(Phase::StompCooldown(0), &mut actions);
                }
            }
            Phase::StompCooldown(t) => {
                if t + 1 >= STOMP_COOLDOWN_TICKS {
                    self.enter(Phase::Cooldown(0), &mut actions);
                } else {
                    self.phase = Phase::StompCooldown(t + 1);
                }
            }
            Phase::Dead => {}
        }
        actions
    }
}
/// `GetDistance` between the two transforms.
fn distance(a: [i32; 2], b: [i32; 2]) -> i32 {
    let (dx, dy) = ((a[0] - b[0]) as i64, (a[1] - b[1]) as i64);
    let d2 = (dx * dx + dy * dy) as u64;
    let mut r = 0u64;
    let mut bit = 1u64 << 62;
    let mut n = d2;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        if n >= r + bit {
            n -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i32
}
/// A box cooked in the art frame (front -x), for a body facing `facing`.
pub fn facing_box(b: [i32; 4], facing: i32, position: [i32; 2]) -> [i32; 4] {
    let (x0, x1) = if facing > 0 {
        (-b[2], -b[0])
    } else {
        (b[0], b[2])
    };
    [
        position[0] + x0,
        position[1] + b[1],
        position[0] + x1,
        position[1] + b[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn senses(me: i32, hero: i32) -> Senses {
        Senses {
            position: [me, 0],
            hero: [hero, 0],
            can_see_hero: true,
            ..Senses::default()
        }
    }
    #[test]
    fn sleeps_until_it_sees_the_hero_in_its_authored_attack_range_then_wakes_and_cools_down() {
        let mut g = HuskGuard::new(0, 1, 7);
        assert_eq!(g.phase(), Phase::Dormant);
        g.tick(senses(0, 5 * ONE));
        assert_eq!(g.phase(), Phase::Dormant);
        // The wide `Alert Range New` alone does not wake it: `Dormant` listens for ATTACK ALERT.
        g.tick(Senses {
            in_alert_range: true,
            ..senses(0, 12 * ONE)
        });
        assert_eq!(g.phase(), Phase::Dormant);
        let s = Senses {
            in_dormant_range: true,
            in_alert_range: true,
            ..senses(0, 5 * ONE)
        };
        g.tick(s);
        assert_eq!(g.phase(), Phase::Wake);
        let done = Senses {
            completed: g.animation(),
            ..s
        };
        g.tick(done);
        assert!(matches!(g.phase(), Phase::Cooldown(_)));
        for _ in 0..COOLDOWN_TICKS {
            g.tick(senses(0, 50 * ONE));
        }
        assert!(matches!(g.phase(), Phase::Idle(_)));
    }
    fn awake(facing: i8) -> HuskGuard {
        let mut g = HuskGuard::new(0, facing, 3);
        g.phase = Phase::Idle(0);
        g
    }
    #[test]
    fn chases_walking_close_running_far_and_turns_to_a_hero_behind() {
        let mut g = awake(1);
        let s = Senses {
            in_alert_range: true,
            ..senses(0, 5 * ONE)
        };
        g.tick(s);
        assert_eq!(g.phase(), Phase::Walk);
        assert_eq!(g.velocity_x(), WALK_SPEED);
        let far = Senses {
            in_alert_range: true,
            ..senses(0, 12 * ONE)
        };
        g.tick(far);
        assert_eq!(g.phase(), Phase::Run);
        assert_eq!(g.velocity_x(), RUN_SPEED);
        let mut g = awake(1);
        g.tick(Senses {
            in_alert_range: true,
            ..senses(0, -5 * ONE)
        });
        assert_eq!(g.phase(), Phase::Turn(AfterTurn::Idle));
        assert_eq!(g.facing(), -1);
    }
    #[test]
    fn attacks_in_range_with_club_or_stomp_and_caps_the_repeats() {
        let (mut clubs, mut stomps, mut run_club, mut run_stomp) = (0, 0, 0u8, 0u8);
        let mut g = awake(1);
        for _ in 0..200 {
            g.phase = Phase::Idle(0);
            g.tick(Senses {
                in_attack_range: true,
                in_alert_range: true,
                ..senses(0, 3 * ONE)
            });
            match g.phase() {
                Phase::Anticipate => {
                    clubs += 1;
                    run_club += 1;
                    run_stomp = 0;
                }
                Phase::StompAntic(_) => {
                    stomps += 1;
                    run_stomp += 1;
                    run_club = 0;
                }
                other => panic!("{other:?}"),
            }
            assert!(run_club <= MAX_CLUBS && run_stomp <= MAX_STOMPS);
        }
        assert!(
            clubs > stomps * 2 && stomps > 20,
            "{clubs} clubs, {stomps} stomps"
        );
    }
    #[test]
    fn a_club_arms_the_swipe_slams_ahead_recoils_back_and_cools_down() {
        let mut g = awake(1);
        g.enter(Phase::Anticipate, &mut Actions::new());
        let a = g.tick(Senses {
            completed: g.animation(),
            ..senses(0, 3 * ONE)
        });
        assert!(a.iter().any(|x| x == Action::Swipe));
        let mut slam = None;
        for _ in 0..ATTACK_TICKS {
            for x in g.tick(senses(0, 3 * ONE)).iter() {
                if let Action::Slam(p) = x {
                    slam = Some(p);
                }
            }
        }
        assert_eq!(slam, Some([SLAM_ORIGIN[0], SLAM_ORIGIN[1]]));
        assert_eq!(g.velocity_x(), -RECOIL_SPEED);
        for _ in 0..RECOIL_TICKS {
            g.tick(senses(0, 3 * ONE));
        }
        assert_eq!(g.phase(), Phase::AttackEnd);
        assert_eq!(g.velocity_x(), 0);
        g.tick(Senses {
            completed: g.animation(),
            ..senses(0, 3 * ONE)
        });
        assert!(matches!(g.phase(), Phase::Cooldown(_)));
    }
    #[test]
    fn a_stomp_hops_back_lands_with_two_waves_and_cools_down() {
        let mut g = awake(1);
        g.enter(Phase::StompAntic(0), &mut Actions::new());
        for _ in 0..STOMP_ANTIC_TICKS {
            g.tick(senses(0, 3 * ONE));
        }
        assert_eq!(g.phase(), Phase::InAir);
        assert_eq!(g.velocity_x(), -JUMP_SPEED);
        let a = g.tick(Senses {
            completed: g.animation(),
            ..senses(0, 3 * ONE)
        });
        assert!(a.iter().any(|x| x == Action::Shockwaves([0, WAVE_Y])));
        g.tick(Senses {
            completed: g.animation(),
            ..senses(0, 3 * ONE)
        });
        assert_eq!(g.phase(), Phase::StompCooldown(0));
        for _ in 0..STOMP_COOLDOWN_TICKS {
            g.tick(senses(0, 50 * ONE));
        }
        assert!(matches!(g.phase(), Phase::Cooldown(_)));
    }
    #[test]
    fn walks_home_after_idling_away_from_its_spot() {
        let mut g = awake(1);
        for _ in 0..IDLE_TICKS {
            g.tick(Senses {
                can_see_hero: false,
                ..senses(10 * ONE, 60 * ONE)
            });
        }
        assert_eq!(g.phase(), Phase::Turn(AfterTurn::Return));
        g.tick(Senses {
            completed: g.animation(),
            can_see_hero: false,
            ..senses(10 * ONE, 60 * ONE)
        });
        assert_eq!(g.phase(), Phase::Return);
        assert_eq!(g.velocity_x(), -WALK_SPEED);
        g.tick(Senses {
            can_see_hero: false,
            ..senses(0, 60 * ONE)
        });
        assert!(matches!(g.phase(), Phase::Idle(_)));
    }
    #[test]
    fn the_swipe_box_is_in_front() {
        let g = awake(1);
        let b = g.swipe_box([0, 0]);
        assert!(b[2] > 4 * ONE && b[0] < 0);
        let g = awake(-1);
        let b = g.swipe_box([0, 0]);
        assert!(b[0] < -4 * ONE);
    }
}
