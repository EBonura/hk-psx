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
        Self {
            x,
            y,
            vx: 0,
            vy: 0,
            gravity: ONE,
            kinematic: false,
        }
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
        panic!(
            "phase machine stalled in {:?} after {} ticks",
            self.boss.phase(),
            self.ticks
        );
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
            matches!(
                f.boss.phase(),
                Phase::Idle | Phase::DeathJumpAntic | Phase::Dead
            )
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
        assert!(
            fight.boss.invincible(),
            "the body refuses the nail while open"
        );
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
    assert_eq!(
        fight.boss.stunned(),
        0,
        "Stun Fail does not increment Stunned Amount"
    );
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
        fight.until(6000, |f| {
            matches!(f.boss.phase(), Phase::RageEnd | Phase::DeathJumpAntic)
        });
        let slams = fight.seen[before..]
            .iter()
            .filter(|a| **a == Action::PlayHitter(Clip::Rage))
            .count();
        assert_eq!(slams, RAGE_SLAMS as usize, "stagger {stagger}");
        let cracked = fight.seen[before..].contains(&Action::CrackFloor);
        assert_eq!(
            cracked,
            stagger == 2,
            "the floor only cracks before the death jump"
        );
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
        assert_ne!(
            fight.boss.phase(),
            Phase::JumpAntic,
            "Determine Jump returns outright once Stunned Amount is two"
        );
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
    // `Run` plays the footstep clip once on the boss's own AudioSource.
    assert_eq!(
        fight
            .seen
            .iter()
            .filter(|a| **a == Action::Effect(Effect::RunStart))
            .count(),
        1
    );
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
    assert!(
        later.ticks < first.ticks,
        "the short plop is 1.2 s against 2.5 s"
    );
    assert_eq!(
        PLOP_LONG_TICKS - PLOP_SHORT_TICKS,
        first.ticks as u16 - later.ticks as u16
    );
}

/// Source health: 65 on the body, restored after every stagger, and 40 on the
/// head. Three head kills, so 195 body damage and 120 head damage in total.
#[test]
fn the_recorded_health_numbers_are_the_source_ones() {
    assert_eq!(HEALTH, 65);
    assert_eq!(HEAD_HEALTH, 40);
    assert_eq!(STAGGERS, 3);
    // HealthManager.NonFatalHit's IL literal (0.2 s); the serialized
    // invulnerableTime (0.25 and 0.15) is read by nothing.
    assert_eq!(INVULNERABLE_TICKS, 12);
    assert_eq!(HEAD_INVULNERABLE_TICKS, 12);
    assert_eq!(CONTACT_DAMAGE, 1);
    assert_eq!(STUN_WINDOW_TICKS, 300);
    assert_eq!(RAGE_SLAMS, 8);
}

#[test]
fn the_turn_writes_its_facing_before_its_clip_plays() {
    // `Turn R` sets Facing Right and SetScale x +1.3 on entry and then holds
    // for the Turn clip, so the body has already turned while it is playing.
    let mut boss = FalseKnight::new(7, true);
    boss.battle_start();
    let mut body = Body::new(0, 20 * ONE);
    let mut senses = Senses {
        self_x: 0,
        hero_x: 8 * ONE,
        distance: 8 * ONE,
        velocity_y: 0,
        velocity_x: 0,
        grounded: false,
        wall_left: false,
        wall_right: false,
        ground_below: false,
    };
    let mut facing_right = false;
    let mut turn_ticks = 0;
    for _ in 0..1200 {
        body.step();
        senses.grounded = body.grounded();
        senses.velocity_y = body.vy;
        senses.self_x = body.x;
        let actions = boss.tick(senses);
        for action in actions.iter() {
            match action {
                Action::Facing(sign) => facing_right = sign > 0,
                Action::Play(Clip::Turn) => {
                    assert!(
                        facing_right,
                        "Turn R writes the facing before its clip plays"
                    );
                    turn_ticks = Clip::Turn.ticks();
                }
                _ => {}
            }
        }
        if turn_ticks != 0 {
            break;
        }
    }
    assert_eq!(
        turn_ticks, TURN_TICKS,
        "Turn holds for the length of its own clip"
    );
}

/// Every `Effect` in a list, in order, so a site that moved shows up as a
/// different sequence rather than as a missing boolean.
fn effects(actions: &[Action]) -> std::vec::Vec<Effect> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Effect(effect) => Some(*effect),
            _ => None,
        })
        .collect()
}

#[test]
fn the_slam_swings_before_the_mace_reaches_the_floor() {
    let mut fight = Fight::new(13, true);
    // `Start Fall` is the first BigShake of the fight, before a tick has run.
    assert!(fight
        .boss
        .battle_start()
        .contains(Action::Effect(Effect::Entrance)));
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    assert_eq!(effects(&fight.seen), [Effect::EntranceLanding], "`State 2`");
    let before = fight.seen.len();
    fight.until(6000, |f| f.boss.phase() == Phase::SlamStrike);
    assert_eq!(
        effects(&fight.seen[before..]).last(),
        Some(&Effect::Swing),
        "`S Attack`"
    );
    let before = fight.seen.len();
    // `Slam` sits between `S Attack` and the recovery that carries the wave.
    fight.until(600, |f| f.boss.phase() == Phase::SlamRecover);
    assert_eq!(effects(&fight.seen[before..]), [Effect::Slam], "`Slam`");
}

/// `Jump`, `S Jump`, `JA Jump` and `Jump 2` each send EnemyKillShake. `JA Jump
/// 2`, the death jump, shares this port's launch arm with three of them and
/// sends nothing, which is the one place a shared arm does not mean a shared
/// effect.
#[test]
fn every_launch_but_the_death_jump_shakes_the_camera() {
    let mut fight = Fight::new(11, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    let before = fight.seen.len();
    fight.until(600, |f| f.boss.phase() == Phase::Airborne);
    assert_eq!(
        effects(&fight.seen[before..]),
        [Effect::JumpShake],
        "`Jump`"
    );
    let before = fight.seen.len();
    // `Land Noise` is the only landing state that plays its voice with no shake.
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    assert_eq!(
        effects(&fight.seen[before..]),
        [Effect::Landing],
        "`Land Noise`"
    );
    for _ in 0..STAGGERS {
        fight.to_opened();
        fight.kill_head();
        fight.through_rage();
    }
    assert_eq!(fight.boss.phase(), Phase::DeathJumpAntic);
    let before = fight.seen.len();
    fight.until(600, |f| f.boss.phase() == Phase::DeathAir);
    // It plays the jump like every other launch, and it is the one launch
    // that plays it with no shake.
    assert_eq!(
        effects(&fight.seen[before..]),
        [Effect::Jump],
        "`JA Jump 2` sends no shake"
    );
}

#[test]
fn the_rage_shakes_once_per_slam_and_its_landing_shakes_not_at_all() {
    let mut fight = Fight::new(19, true);
    fight.boss.battle_start();
    fight.until(600, |f| f.boss.phase() == Phase::Landing);
    fight.to_opened();
    fight.kill_head();
    // `Jump 2` launches the rage, and the state it lands in plays no clip and
    // sends no shake, so the next effect is the first `Rage Slam`.
    fight.until(4000, |f| f.boss.phase() == Phase::RageAir);
    let before = fight.seen.len();
    fight.until(6000, |f| {
        matches!(f.boss.phase(), Phase::RageEnd | Phase::DeathJumpAntic)
    });
    assert_eq!(
        effects(&fight.seen[before..]),
        std::vec::Vec::from([Effect::RageSlam; RAGE_SLAMS as usize])
    );
}

#[test]
fn the_two_child_hitboxes_sit_where_the_source_colliders_do() {
    // Both are already multiplied by the body's 1.3 transform scale, and both
    // are in the source's facing-right frame. The Head is the 40 hp target the
    // armour exposes and it sits above the body box rather than inside it; the
    // Hitter reaches nearly eight units ahead, which is the ground the slam
    // covers without the body moving.
    assert!(
        HEAD_BOX[1] > -2 * ONE && HEAD_BOX[3] > 0,
        "the head is above the armour"
    );
    assert_eq!(HEAD_BOX[0], -HEAD_BOX[2], "the head box is symmetric in x");
    assert!(
        HITTER_BOX[0] > 0 && HITTER_BOX[2] > 7 * ONE,
        "the hitter reaches ahead of the body"
    );
    assert!(
        HITTER_BOX[1] < HITTER_BOX[3] && HITTER_BOX[3] < 0,
        "and below the transform"
    );
}

/// Source `summon` on `FK Barrel Summon`: the loop that turns one SUMMON into a
/// spaced-out burst of `Falling Barrel`s.
#[test]
fn a_burst_spaces_every_barrel_and_never_starts_on_the_frame_it_was_asked_for() {
    let mut summon = Summon::new(7);
    summon.summon(4);
    assert_eq!(summon.remaining(), 4);
    // `Determine Spawns` holds the same WaitRandom as `Spawn`, so there is a
    // gap in front of the first barrel as well as between the later ones.
    let mut spawns = std::vec::Vec::new();
    let mut gap = 0u16;
    for _ in 0..600 {
        gap += 1;
        if let Some(x) = summon.tick() {
            spawns.push((gap, x));
            gap = 0;
        }
    }
    assert_eq!(
        spawns.len(),
        4,
        "the burst is exactly the count it was given"
    );
    for (gap, x) in spawns {
        assert!(
            (BARREL_GAP_TICKS[0]..=BARREL_GAP_TICKS[1]).contains(&gap),
            "every gap is the source WaitRandom, got {gap}"
        );
        assert!(
            (BARREL_SPAWN_X[0]..=BARREL_SPAWN_X[1]).contains(&x),
            "every barrel lands inside Summon Min..Summon Max"
        );
    }
    assert_eq!(summon.remaining(), 0);
    assert_eq!(summon.tick(), None, "an emptied summoner is back in Idle");
}

#[test]
fn a_summon_during_a_burst_is_dropped_the_way_the_state_machine_drops_it() {
    // Only `Idle` declares the SUMMON transition; `Determine Spawns`, `Spawn`
    // and `Check Spawns` do not, so a second send while the loop is running
    // reaches nothing rather than restarting or adding to the count.
    let mut summon = Summon::new(11);
    summon.summon(3);
    for _ in 0..BARREL_GAP_TICKS[1] + 1 {
        summon.tick();
    }
    assert_eq!(summon.remaining(), 2);
    summon.summon(8);
    assert_eq!(
        summon.remaining(),
        2,
        "the second SUMMON found no state that answers it"
    );
    let mut spawned = 1;
    for _ in 0..600 {
        if summon.tick().is_some() {
            spawned += 1;
        }
    }
    assert_eq!(spawned, 3);
}

#[test]
fn the_rage_asks_for_exactly_the_pool_and_the_phase_tables_stay_inside_it() {
    // `R Attack Antic` sets Rages to eight and sends one SUMMON of eight, which
    // is why `FK Barrel Summon`'s PersonalObjectPool reserves eight and why the
    // guest's projectile pool is sized from this constant.
    assert_eq!(BARREL_POOL, RAGE_SLAMS as usize);
    for table in [JUMP_BARRELS, SLAM_BARRELS] {
        for [low, high] in table {
            assert!(low <= high && high as usize <= BARREL_POOL);
        }
    }
}
