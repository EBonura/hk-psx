use hk_sim::false_knight::*;
use hk_sim::ONE;

/// Test scaffolding, not a source value: the guest's real body is the bounded
/// terrain solver. All these numbers have to do is rise and fall plausibly so
/// the phase machine sees the contacts the source's FSM waits on.
const HARNESS_GRAVITY: i32 = 60 * ONE;
const ARENA_FLOOR: i32 = 0;

struct Body {
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    gravity: i32,
    kinematic: bool,
}
impl Body {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y, vx: 0, vy: 0, gravity: ONE, kinematic: false }
    }
    fn step(&mut self) {
        if self.kinematic {
            return;
        }
        self.vy -= (((HARNESS_GRAVITY as i64 * self.gravity as i64) >> 16) / 60) as i32;
        self.y += self.vy / 60;
        self.x += self.vx / 60;
        if self.y <= ARENA_FLOOR {
            self.y = ARENA_FLOOR;
            if self.vy < 0 {
                self.vy = 0;
            }
        }
    }
    fn grounded(&self) -> bool {
        self.y <= ARENA_FLOOR
    }
}

struct Fight {
    boss: FalseKnight,
    body: Body,
    hero_x: i32,
    /// Every action the drive loop has seen, for the assertions below.
    seen: std::vec::Vec<Action>,
    ticks: u32,
}
impl Fight {
    fn new(seed: u32, first_plop: bool) -> Self {
        Self {
            boss: FalseKnight::new(seed, first_plop),
            body: Body::new(26 * ONE, 12 * ONE),
            hero_x: 20 * ONE,
            seen: std::vec::Vec::new(),
            ticks: 0,
        }
    }
    fn step(&mut self) -> Actions {
        self.body.step();
        let senses = Senses {
            self_x: self.body.x,
            hero_x: self.hero_x,
            distance: (self.hero_x - self.body.x).abs(),
            velocity_y: self.body.vy,
            velocity_x: self.body.vx,
            grounded: self.body.grounded(),
            wall_left: false,
            wall_right: false,
            ground_below: self.body.y <= FALL_RAY_DISTANCE,
        };
        let actions = self.boss.tick(senses);
        for action in actions.iter() {
            self.seen.push(action);
            match action {
                Action::Velocity([x, y]) => {
                    self.body.vx = x;
                    self.body.vy = y;
                }
                Action::VelocityX(x) => self.body.vx = x,
                Action::Gravity(g) => self.body.gravity = g,
                Action::Kinematic(k) => self.body.kinematic = k,
                _ => {}
            }
        }
        self.ticks += 1;
        actions
    }
    /// Run until the predicate holds, failing rather than spinning forever.
    fn until(&mut self, limit: u32, mut done: impl FnMut(&Fight) -> bool) {
        for _ in 0..limit {
            if done(self) {
                return;
            }
            self.step();
        }
        panic!("phase machine stalled in {:?} after {} ticks", self.boss.phase(), self.ticks);
    }
    fn to_opened(&mut self) {
        let actions = self.boss.body_reached_zero();
        for action in actions.iter() {
            match action {
                Action::Velocity([x, y]) => {
                    self.body.vx = x;
                    self.body.vy = y;
                }
                Action::Gravity(g) => self.body.gravity = g,
                _ => {}
            }
        }
        assert_eq!(self.boss.phase(), Phase::StunRoll);
        self.until(1200, |f| f.boss.phase() == Phase::Opened);
    }
    fn kill_head(&mut self) {
        let actions = self.boss.head_reached_zero();
        for action in actions.iter() {
            if let Action::Kinematic(k) = action {
                self.body.kinematic = k;
            }
        }
    }
    /// Advance the rage that follows a stagger, back to ordinary combat or to
    /// the death jump.
    fn through_rage(&mut self) {
        self.until(4000, |f| {
            matches!(f.boss.phase(), Phase::Idle | Phase::DeathJumpAntic | Phase::Dead)
        });
    }
}

#[test]
fn the_entrance_clears_the_room_before_the_scripted_opening_jump() {
    let mut fight = Fight::new(7, true);
    assert_eq!(fight.boss.phase(), Phase::Dormant);
    for _ in 0..600 {
        assert!(fight.step().is_empty(), "a dormant boss must do nothing");
    }
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    assert!(fight.seen.contains(&Action::KillAllEnemies));
    fight.until(600, |f| f.boss.phase() == Phase::FirstIdle);
    // `First Idle` waits 1.5 s and then jumps without consulting Move Choice.
    for _ in 0..FIRST_IDLE_TICKS - 1 {
        fight.step();
        assert_eq!(fight.boss.phase(), Phase::FirstIdle);
    }
    fight.step();
    assert_eq!(fight.boss.phase(), Phase::JumpAntic);
}

#[test]
fn three_head_kills_advance_the_phase_table_and_end_the_fight() {
    let mut fight = Fight::new(11, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    assert_eq!(fight.boss.table(), 0);
    assert_eq!(IDLE_TICKS[0], [60, 60], "phase one idles for a flat second");

    for expected in 1..=STAGGERS {
        fight.to_opened();
        assert!(fight.boss.head_exposed());
        assert!(fight.boss.invincible(), "the body refuses the nail while open");
        fight.kill_head();
        assert_eq!(fight.boss.stunned(), expected);
        fight.through_rage();
        if expected < STAGGERS {
            assert_eq!(fight.boss.phase(), Phase::Idle);
            assert_eq!(fight.boss.table(), expected as usize);
        }
    }
    // The third rage ends in the death jump rather than back in the fight.
    assert_eq!(fight.boss.phase(), Phase::DeathJumpAntic);
    fight.until(2000, |f| f.boss.phase() == Phase::DeathOpened);
    assert!(fight.seen.contains(&Action::BreakFloor));
    fight.kill_head();

    // `Death Anim Start` through `Death Head Land`, then the arena is told.
    let expected: u32 = DEATH_TAIL_TICKS.iter().map(|&t| t as u32).sum();
    let start = fight.ticks;
    fight.until(expected + 10, |f| f.boss.phase() == Phase::Dead);
    assert_eq!(fight.ticks - start, expected);
    assert!(fight.seen.contains(&Action::Died));
}

#[test]
fn a_stagger_the_player_lets_time_out_costs_no_phase() {
    let mut fight = Fight::new(3, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    fight.to_opened();
    for _ in 0..STUN_WINDOW_TICKS - 1 {
        fight.step();
        assert_eq!(fight.boss.phase(), Phase::Opened);
    }
    fight.step();
    assert_eq!(fight.boss.phase(), Phase::Recovering);
    assert_eq!(fight.boss.stunned(), 0, "Stun Fail does not increment Stunned Amount");
    fight.until(200, |f| f.boss.phase() == Phase::Idle);
    assert_eq!(fight.boss.table(), 0);
}

#[test]
fn every_head_hit_restarts_the_five_second_window() {
    let mut fight = Fight::new(5, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    fight.to_opened();
    for _ in 0..STUN_WINDOW_TICKS - 30 {
        fight.step();
    }
    fight.boss.head_hit();
    assert_eq!(fight.boss.phase(), Phase::StunHit);
    for _ in 0..STUN_HIT_TICKS {
        fight.step();
    }
    assert_eq!(fight.boss.phase(), Phase::Opened);
    // A full fresh window, not the 30 ticks that were left.
    for _ in 0..STUN_WINDOW_TICKS - 1 {
        fight.step();
        assert_eq!(fight.boss.phase(), Phase::Opened);
    }
}

#[test]
fn the_rage_runs_eight_slams_and_only_the_third_phase_cracks_the_floor() {
    for stagger in 1..=2u8 {
        let mut fight = Fight::new(17 + stagger as u32, true);
        fight.boss.battle_start();
        fight.until(600, |f| f.boss.phase() == Phase::Landing);
        for _ in 0..stagger {
            fight.to_opened();
            fight.kill_head();
            if fight.boss.stunned() < stagger {
                fight.through_rage();
            }
        }
        let before = fight.seen.len();
        fight.until(4000, |f| f.boss.phase() == Phase::RageSlam);
        fight.until(6000, |f| matches!(f.boss.phase(), Phase::RageEnd | Phase::DeathJumpAntic));
        let slams = fight.seen[before..]
            .iter()
            .filter(|a| **a == Action::PlayHitter(Clip::Rage))
            .count();
        assert_eq!(slams, RAGE_SLAMS as usize, "stagger {stagger}");
        let cracked = fight.seen[before..].contains(&Action::CrackFloor);
        assert_eq!(cracked, stagger == 2, "the floor only cracks before the death jump");
    }
}

#[test]
fn the_third_phase_stops_jumping_and_summons_more_barrels() {
    assert_eq!(JUMP_BARRELS[0], [0, 0], "phase one summons nothing");
    assert_eq!(SLAM_BARRELS[2], [3, 4]);
    let mut fight = Fight::new(23, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    for _ in 0..2 {
        fight.to_opened();
        fight.kill_head();
        fight.through_rage();
    }
    assert_eq!(fight.boss.stunned(), 2);
    let before = fight.ticks;
    for _ in 0..3000 {
        fight.step();
        assert_ne!(fight.boss.phase(), Phase::JumpAntic,
                   "Determine Jump returns outright once Stunned Amount is two");
    }
    assert!(fight.ticks > before);
    let summons: u32 = fight
        .seen
        .iter()
        .filter_map(|a| match a {
            Action::SummonBarrels(n) => Some(*n as u32),
            _ => None,
        })
        .sum();
    assert!(summons > 0, "phase three summons barrels from both attacks");
}

/// The chase is not one of the three weighted attacks: it is the every-frame
/// distance test in front of them, and it feeds the jump attack directly
/// without spending the in-a-row budget.
#[test]
fn a_distant_hero_is_chased_rather_than_attacked() {
    let mut fight = Fight::new(29, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    fight.hero_x = fight.body.x + 30 * ONE;
    fight.until(600, |f| f.boss.phase() == Phase::RunAntic);
    fight.until(600, |f| f.boss.phase() == Phase::Run);
    assert!(fight.seen.contains(&Action::Play(Clip::Run)));
    // Closing to under fourteen units hands straight over to the jump attack.
    fight.hero_x = fight.body.x + 5 * ONE;
    fight.until(600, |f| f.boss.phase() == Phase::JumpAttackAntic);
}

#[test]
fn the_first_plop_is_longer_and_only_happens_once_per_save() {
    let mut first = Fight::new(31, false);
    first.boss.battle_start();
    first.until(600, |f| f.boss.phase() == Phase::Landing);
    first.to_opened();
    assert!(first.seen.contains(&Action::SetFirstPlop));

    let mut later = Fight::new(31, true);
    later.boss.battle_start();
    later.until(600, |f| f.boss.phase() == Phase::Landing);
    later.to_opened();
    assert!(!later.seen.contains(&Action::SetFirstPlop));
    assert!(later.ticks < first.ticks, "the short plop is 1.2 s against 2.5 s");
    assert_eq!(PLOP_LONG_TICKS - PLOP_SHORT_TICKS, first.ticks as u16 - later.ticks as u16);
}

/// Source health: 65 on the body, restored after every stagger, and 40 on the
/// head. Three head kills, so 195 body damage and 120 head damage in total.
#[test]
fn the_recorded_health_numbers_are_the_source_ones() {
    assert_eq!(HEALTH, 65);
    assert_eq!(HEAD_HEALTH, 40);
    assert_eq!(STAGGERS, 3);
    assert_eq!(INVULNERABLE_TICKS, 15);
    assert_eq!(HEAD_INVULNERABLE_TICKS, 9);
    assert_eq!(CONTACT_DAMAGE, 1);
    assert_eq!(STUN_WINDOW_TICKS, 300);
    assert_eq!(RAGE_SLAMS, 8);
}
