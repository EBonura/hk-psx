//! Source-derived Hatcher (`Hatcher` FSM) and Hatcher Baby (`Control` FSM).
//!
//! The Hatcher is the first enemy in this port whose behaviour needs another
//! actor to change state at runtime. It does not create one: the source parks a
//! fixed set of babies as children of a scene-wide `Hatcher Cage`, and `Fire`
//! moves one of them to the Hatcher, sends it SPAWN and unparents it. Death
//! puts it back. So the pool is a fixed reservation and a release is a state
//! change, never an allocation. `Release` is the whole of the contract this
//! module states; `game/src/enemies.rs` owns the reservation and honours it.
//!
//! The caller owns physics (a gravity-free dynamic body against terrain), the
//! alert circle, Recoil, HealthManager hits and Geo, exactly as it does for the
//! Aspid. The source 50 Hz fixed steps run from an accumulator inside the 60 Hz
//! tick and `Random.Range` is a deterministic per-actor sequence.
//!
//! Evidence: docs/HATCHER.md, and `ChaseObject::DoBuzz` in
//! .hkpsx/vengefly/actions.il for the baby's spread.
use crate::buzz::{clamp, distance_fly_height, IdleBuzz};
use crate::ONE;

/// `Alert Range New`: a 0.5 CircleCollider2D under a uniform local scale of
/// 15.608528137207031, so the circle is 7.8042640686 units.
///
/// `enemies.rs::advance_aspid` writes the same authored circle as 511463, five
/// Q16 units short of this rounding. The two are the same source object; the
/// difference is 0.00008 world units and neither number is derived from the
/// other, so this one is left exact rather than matched to the Aspid's.
pub const ALERT_RADIUS: i32 = 511468;
/// `DistanceFly(distance 6, speedMax 3.5, acceleration .1, height 3.5)`.
pub const FLY_DISTANCE: i32 = 6 * ONE;
pub const FLY_HEIGHT: i32 = 229376; // 3.5
pub const FLY_SPEED_MAX: i32 = 229376; // 3.5
pub const FLY_ACCELERATION: i32 = 6554; // .1 per fixed step
/// `WaitRandom(2, 3)` between one `Hatched Max Check` and the next.
pub const FLY_WAIT: (u16, u16) = (120, 180);
/// `Fire Anticipate`'s `Wait 0.335`.
pub const ANTICIPATE_TICKS: u16 = 20;
/// The `Fire` clip: eight frames at 15 fps. `Fire Anticipate` starts it and
/// `Fire`'s `Tk2dWatchAnimationEvents` waits for the same clip to complete, so
/// the state after the anticipation lasts what is left of it.
pub const FIRE_CLIP_TICKS: u16 = 32;
/// `Tk2dPlayFrame 2` on the six-frame 12 fps `Fly` clip, in 60 Hz ticks.
pub const FLY_START_TICKS: u32 = 10;
/// `Fire`: `Spawn Y` is the Hatcher's own y less one unit, and the released
/// baby leaves with `SetVelocity2d(y = -5)`.
pub const RELEASE_DROP: i32 = ONE;
pub const RELEASE_VELOCITY_Y: i32 = -5 * ONE;

/// `ChaseObject(speedMax 5, acceleration .1, targetSpread 1.5,
/// spreadResetTimeMin 1, spreadResetTimeMax 2)` on the baby.
pub const CHASE_SPEED_MAX: i32 = 5 * ONE;
pub const CHASE_ACCELERATION: i32 = 6554; // .1 per fixed step
pub const CHASE_SPREAD: i32 = 98304; // 1.5
/// `spreadResetTime` counts `Time.deltaTime` inside `OnFixedUpdate`, so it
/// advances one 50 Hz step at a time: 1 to 2 seconds is 50 to 100 steps.
pub const SPREAD_RESET_STEPS: (u16, u16) = (50, 100);
/// The baby's `FaceDirection(pauseBetweenTurns, pauseTime 0.4)`.
pub const FACE_PAUSE_TICKS: u16 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Idle`: roam within a unit of where it woke up until `Alert Range`.
    Idle,
    /// `Distance Fly`, which is also where `Hatched Max Check` resolves.
    DistanceFly,
    /// `Fire Anticipate`: velocity zero, the Fire clip, 0.335 s.
    Anticipate,
    /// `Fire`: the release already happened on entry; this is the rest of the clip.
    Fire,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Fly,
    Fire,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Velocity([i32; 2]),
    Play(Clip, u32),
    /// Sprite facing, +1 right (source scale.x negative) or -1 left.
    Facing(i32),
    /// `Fire`'s `SetPosition` + `SPAWN` + `SetVelocity2d` on the cage child it
    /// drew. The caller picks which reserved baby answers it; the source draws
    /// uniformly with `GetRandomChild` and the guest's order is its own.
    Release {
        position: [i32; 2],
        velocity: [i32; 2],
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 4],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 4],
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
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// The `alert_range` FSM's `Alert Range` bool: hero inside the 7.804
    /// circle and the straight line to it unobstructed.
    pub alert_range: bool,
    /// `GetChildCount(Cage)`, which is how many babies are still parked.
    pub cage_children: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hatcher {
    phase: Phase,
    velocity: [i32; 2],
    buzz: IdleBuzz,
    timer: u16,
    facing: i32,
    fixed_accumulator: u8,
    rng: u32,
}
impl Hatcher {
    pub fn new(position: [i32; 2], seed: u32) -> Self {
        Self {
            phase: Phase::Idle,
            velocity: [0; 2],
            buzz: IdleBuzz::new(position),
            timer: 0,
            facing: -1,
            fixed_accumulator: 0,
            rng: seed,
        }
    }
    /// FSM variable `startAlert`: `Idle`'s first BoolTest sends ALERT at once.
    pub fn new_alert(position: [i32; 2], seed: u32) -> Self {
        let mut hatcher = Self::new(position, seed);
        let mut out = Actions::new();
        hatcher.begin_distance_fly(&mut out);
        hatcher
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn velocity(self) -> [i32; 2] {
        self.velocity
    }
    pub fn facing(self) -> i32 {
        self.facing
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
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    /// Neither `FaceDirection` (Idle) nor `FaceObject` (Distance Fly) pauses on
    /// this placement, so the sprite follows the sign on the frame it changes.
    fn face(&mut self, want: i32, out: &mut Actions) {
        if want != self.facing {
            self.facing = want;
            out.push(Action::Facing(want));
        }
    }
    fn begin_distance_fly(&mut self, out: &mut Actions) {
        self.phase = Phase::DistanceFly;
        self.timer = FLY_WAIT.0 + (self.random() % (FLY_WAIT.1 - FLY_WAIT.0 + 1) as u32) as u16;
        out.push(Action::Play(Clip::Fly, FLY_START_TICKS));
    }
    /// `Fire`'s own actions, which run on the frame the state is entered.
    fn fire(&mut self, senses: Senses, out: &mut Actions) {
        // `GetRandomChild` + `GameObjectIsNull` -> CANCEL. The cage can have
        // emptied while the anticipation played, and the source answers that by
        // going back to Distance Fly without firing.
        if senses.cage_children == 0 {
            self.begin_distance_fly(out);
            return;
        }
        self.phase = Phase::Fire;
        self.timer = FIRE_CLIP_TICKS - ANTICIPATE_TICKS;
        out.push(Action::Release {
            position: [senses.position[0], senses.position[1] - RELEASE_DROP],
            velocity: [0, RELEASE_VELOCITY_Y],
        });
    }
    /// One nominal 60 Hz frame.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Dead {
            return out;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        match self.phase {
            Phase::Idle => {
                if senses.alert_range {
                    self.begin_distance_fly(&mut out);
                    return out;
                }
                if fixed {
                    let mut v = self.velocity;
                    let mut buzz = self.buzz;
                    buzz.step(senses.position, &mut v, &mut |lo, hi| self.range(lo, hi));
                    self.buzz = buzz;
                    self.velocity = v;
                    out.push(Action::Velocity(self.velocity));
                }
                let want = if self.velocity[0] > 0 { 1 } else { -1 };
                self.face(want, &mut out);
            }
            Phase::DistanceFly => {
                if fixed {
                    let mut v = self.velocity;
                    distance_fly_height(
                        senses.position,
                        senses.hero,
                        FLY_DISTANCE,
                        FLY_HEIGHT,
                        FLY_SPEED_MAX,
                        FLY_ACCELERATION,
                        &mut v,
                    );
                    self.velocity = v;
                    out.push(Action::Velocity(self.velocity));
                }
                let want = if senses.hero[0] > senses.position[0] {
                    1
                } else {
                    -1
                };
                self.face(want, &mut out);
                self.timer -= 1;
                if self.timer == 0 {
                    // `Hatched Max Check` is a one-frame state: the live gate is
                    // `GetChildCount(Cage) > 0`, not the disabled `Spawned` cap.
                    if senses.cage_children == 0 {
                        self.begin_distance_fly(&mut out);
                    } else {
                        self.phase = Phase::Anticipate;
                        self.timer = ANTICIPATE_TICKS;
                        self.velocity = [0; 2];
                        out.push(Action::Velocity(self.velocity));
                        out.push(Action::Play(Clip::Fire, 0));
                    }
                }
            }
            Phase::Anticipate => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.fire(senses, &mut out);
                }
            }
            Phase::Fire => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.begin_distance_fly(&mut out);
                }
            }
            Phase::Dead => {}
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BabyPhase {
    /// `Inert`: parked in the cage at zero velocity, waiting for SPAWN.
    Inert,
    Chase,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BabyAction {
    Velocity([i32; 2]),
    Facing(i32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BabyActions {
    values: [Option<BabyAction>; 2],
    count: u8,
}
impl BabyActions {
    const fn new() -> Self {
        Self {
            values: [None; 2],
            count: 0,
        }
    }
    fn push(&mut self, action: BabyAction) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = BabyAction> + '_ {
        self.values[..self.count as usize]
            .iter()
            .map(|a| a.unwrap())
    }
}
/// The Hatcher Baby's `Control` FSM.
///
/// `Init` and `Death` are not states this runs: `Init` only caches the cage and
/// the hero, and `Death` restores hp, clears the dead flag and reparents, which
/// is the caller's reservation bookkeeping rather than behaviour. What is left
/// is `Inert` and `Chase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baby {
    phase: BabyPhase,
    velocity: [i32; 2],
    spread: [i32; 2],
    spread_left: u16,
    facing: i32,
    face_pause: u16,
    fixed_accumulator: u8,
    rng: u32,
}
impl Baby {
    pub const fn new(seed: u32) -> Self {
        Self {
            phase: BabyPhase::Inert,
            velocity: [0; 2],
            spread: [0; 2],
            spread_left: 0,
            facing: -1,
            face_pause: 0,
            fixed_accumulator: 0,
            rng: seed,
        }
    }
    pub fn phase(self) -> BabyPhase {
        self.phase
    }
    pub fn velocity(self) -> [i32; 2] {
        self.velocity
    }
    pub fn facing(self) -> i32 {
        self.facing
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    /// SPAWN: `Chase` unparents the body and starts with the velocity the
    /// Hatcher wrote. `ChaseObject::OnEnter` calls `DoBuzz` once, and its timer
    /// and `spreadResetTime` both start at zero, so the first call draws a
    /// spread before it accelerates.
    pub fn release(&mut self, velocity: [i32; 2]) {
        self.phase = BabyPhase::Chase;
        self.velocity = velocity;
        self.spread_left = 0;
        self.face_pause = 0;
    }
    /// The `Death` state, which recycles rather than removing: the caller
    /// restores hp and re-parks the body, and this returns the controller to
    /// `Inert` so the next SPAWN finds it the way `Init` left it.
    pub fn park(&mut self) {
        self.phase = BabyPhase::Inert;
        self.velocity = [0; 2];
        self.spread = [0; 2];
        self.spread_left = 0;
        self.face_pause = 0;
    }
    fn chase(&mut self, position: [i32; 2], hero: [i32; 2]) {
        if self.spread_left == 0 {
            self.spread = [
                self.range(-CHASE_SPREAD, CHASE_SPREAD),
                self.range(-CHASE_SPREAD, CHASE_SPREAD),
            ];
            self.spread_left = SPREAD_RESET_STEPS.0
                + (self.random() % (SPREAD_RESET_STEPS.1 - SPREAD_RESET_STEPS.0 + 1) as u32) as u16;
        } else {
            self.spread_left -= 1;
        }
        let mut v = self.velocity;
        for axis in 0..2 {
            let target = hero[axis] + self.spread[axis];
            v[axis] += if position[axis] < target {
                CHASE_ACCELERATION
            } else {
                -CHASE_ACCELERATION
            };
        }
        clamp(&mut v, CHASE_SPEED_MAX);
        self.velocity = v;
    }
    /// One nominal 60 Hz frame. A parked baby is not ticked at all.
    pub fn tick(&mut self, position: [i32; 2], hero: [i32; 2]) -> BabyActions {
        let mut out = BabyActions::new();
        if self.phase == BabyPhase::Inert {
            return out;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
            self.chase(position, hero);
            out.push(BabyAction::Velocity(self.velocity));
        }
        if self.face_pause > 0 {
            self.face_pause -= 1;
        } else {
            let want = if self.velocity[0] > 0 { 1 } else { -1 };
            if want != self.facing {
                self.facing = want;
                self.face_pause = FACE_PAUSE_TICKS;
                out.push(BabyAction::Facing(want));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HERO: [i32; 2] = [40 * ONE, 30 * ONE];
    fn senses(position: [i32; 2], alert_range: bool, cage_children: u16) -> Senses {
        Senses {
            position,
            hero: HERO,
            alert_range,
            cage_children,
        }
    }
    #[test]
    fn idle_roams_until_the_alert_bool_and_never_returns_to_idle() {
        let start = [10 * ONE, 10 * ONE];
        let mut hatcher = Hatcher::new(start, 7);
        for _ in 0..600 {
            hatcher.tick(senses(start, false, 15));
            assert_eq!(hatcher.phase(), Phase::Idle);
        }
        // IdleBuzz stays inside its own roaming speed cap.
        assert!(hatcher
            .velocity()
            .iter()
            .all(|v| v.abs() <= crate::buzz::IDLE_SPEED_MAX));
        hatcher.tick(senses(start, true, 15));
        assert_eq!(hatcher.phase(), Phase::DistanceFly);
        // The source Idle has one transition out and nothing sends it back.
        for _ in 0..1200 {
            hatcher.tick(senses(start, false, 15));
            assert_ne!(hatcher.phase(), Phase::Idle);
        }
    }
    #[test]
    fn an_empty_cage_keeps_flying_and_never_releases() {
        let start = [10 * ONE, 10 * ONE];
        let mut hatcher = Hatcher::new_alert(start, 3);
        assert_eq!(hatcher.phase(), Phase::DistanceFly);
        for _ in 0..2000 {
            for action in hatcher.tick(senses(start, true, 0)).iter() {
                assert!(
                    !matches!(action, Action::Release { .. }),
                    "released from an empty cage"
                );
            }
            assert_eq!(hatcher.phase(), Phase::DistanceFly);
        }
    }
    #[test]
    fn a_full_cage_releases_one_baby_a_cycle_below_the_hatcher() {
        let start = [10 * ONE, 10 * ONE];
        let mut hatcher = Hatcher::new_alert(start, 11);
        let mut released = 0;
        let mut seen_anticipate = false;
        for _ in 0..1200 {
            for action in hatcher.tick(senses(start, true, 15)).iter() {
                if let Action::Release { position, velocity } = action {
                    released += 1;
                    assert_eq!(position, [start[0], start[1] - ONE]);
                    assert_eq!(velocity, [0, -5 * ONE]);
                    assert_eq!(hatcher.phase(), Phase::Fire);
                }
            }
            seen_anticipate |= hatcher.phase() == Phase::Anticipate;
        }
        assert!(seen_anticipate);
        // WaitRandom(2, 3) plus 0.335 s plus the rest of the Fire clip is at
        // least 140 ticks a cycle, so 1200 ticks cannot release more than nine.
        assert!((4..=9).contains(&released), "released {released}");
    }
    #[test]
    fn a_cage_that_empties_during_the_anticipation_cancels_the_shot() {
        let start = [10 * ONE, 10 * ONE];
        let mut hatcher = Hatcher::new_alert(start, 5);
        while hatcher.phase() != Phase::Anticipate {
            hatcher.tick(senses(start, true, 1));
        }
        let mut released = 0;
        while hatcher.phase() == Phase::Anticipate {
            for action in hatcher.tick(senses(start, true, 0)).iter() {
                released += u32::from(matches!(action, Action::Release { .. }));
            }
        }
        assert_eq!(released, 0);
        assert_eq!(hatcher.phase(), Phase::DistanceFly);
    }
    #[test]
    fn distance_fly_holds_its_authored_separation_and_height() {
        let mut hatcher = Hatcher::new_alert([0, 0], 19);
        let mut position = [0, 0];
        for _ in 0..600 {
            hatcher.tick(senses(position, true, 0));
            let v = hatcher.velocity();
            position = [position[0] + v[0] / 60, position[1] + v[1] / 60];
            assert!(v.iter().all(|value| value.abs() <= FLY_SPEED_MAX));
        }
        // targetsHeight: y converges on the hero's height plus 3.5 units.
        assert!(
            (position[1] - (HERO[1] + FLY_HEIGHT)).abs() < ONE,
            "y {}",
            position[1]
        );
        let dx = (position[0] - HERO[0]).abs();
        assert!(
            (FLY_DISTANCE - ONE..=FLY_DISTANCE + ONE).contains(&dx),
            "dx {dx}"
        );
    }
    #[test]
    fn a_parked_baby_costs_nothing_and_a_released_one_chases() {
        let mut baby = Baby::new(23);
        let parked = baby;
        for _ in 0..600 {
            assert!(baby
                .tick([100 * ONE, 100 * ONE], HERO)
                .iter()
                .next()
                .is_none());
        }
        assert_eq!(baby, parked);
        baby.release([0, -5 * ONE]);
        assert_eq!(baby.phase(), BabyPhase::Chase);
        assert_eq!(baby.velocity(), [0, -5 * ONE]);
        let mut position = [30 * ONE, 40 * ONE];
        let start = (position[0] - HERO[0])
            .abs()
            .max((position[1] - HERO[1]).abs());
        let mut far = start;
        for _ in 0..600 {
            baby.tick(position, HERO);
            let v = baby.velocity();
            assert!(v.iter().all(|value| value.abs() <= CHASE_SPEED_MAX));
            position = [position[0] + v[0] / 60, position[1] + v[1] / 60];
            far = (position[0] - HERO[0])
                .abs()
                .max((position[1] - HERO[1]).abs());
        }
        // ChaseObject has no braking term, so it closes and then orbits the
        // spread target rather than settling on the hero.
        assert!(
            start >= 10 * ONE && far < 5 * ONE,
            "start {start} far {far}"
        );
    }
    #[test]
    fn a_recycled_baby_starts_from_the_state_init_left() {
        let mut baby = Baby::new(29);
        baby.release([0, -5 * ONE]);
        for _ in 0..300 {
            baby.tick([30 * ONE, 40 * ONE], HERO);
        }
        baby.park();
        // `Death` restores what `Inert` needs and nothing else: the transform's
        // facing and the random sequence carry over the way the source's do, so
        // a recycled baby does not repeat the one before it.
        assert_eq!(baby.phase(), BabyPhase::Inert);
        assert_eq!(
            (
                baby.velocity(),
                baby.spread,
                baby.spread_left,
                baby.face_pause
            ),
            ([0; 2], [0; 2], 0, 0)
        );
        assert_ne!(baby.rng, Baby::new(29).rng);
    }
    #[test]
    fn the_spread_is_redrawn_inside_its_authored_window() {
        let mut baby = Baby::new(31);
        baby.release([0; 2]);
        let mut spreads = 0;
        let mut last = baby.spread;
        for _ in 0..1200 {
            baby.tick([30 * ONE, 40 * ONE], HERO);
            if baby.spread != last {
                spreads += 1;
                last = baby.spread;
                assert!(baby.spread.iter().all(|s| s.abs() <= CHASE_SPREAD));
            }
        }
        // 1000 fixed steps over a 50 to 100 step window.
        assert!((10..=20).contains(&spreads), "spreads {spreads}");
    }
}
