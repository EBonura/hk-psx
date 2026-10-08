//! Offline False Knight fight simulator and input-tape search.
//!
//! `tools/boss_sim.py` builds and drives this; that wrapper is where the cooked
//! world it reads comes from, and `tests/test_boss_sim.py` is what catches this
//! drifting out of step with the guest.
//!
//! # What it is
//!
//! `shared/hk-sim`'s `Player`, `Nail`, `NailResponse`, `Vitals`, `Focus`,
//! `false_knight::FalseKnight`, `false_knight::Summon` and `boss::Arena`, driven
//! through the same per-tick call order as `frame::simulate` and the False
//! Knight branch of `enemies.rs`'s `EnemyWorld::tick`, against the cooked
//! Crossroads_10 room packs. One 4,600-poll fight simulates in about 20 ms, so a
//! beam search over input segments can author a boss tape in seconds instead of
//! at one 85-second `tools/replay_cue.py` round trip per attempt.
//!
//! # What it is not
//!
//! **It models the fight, not the game.** There is no title screen, no CD, no
//! disc, no renderer, no audio, no camera, no scene load, no death respawn, no
//! save record, no pause menu, no cheats, no Geo, no Lifeblood, no breakables
//! and no actor but the boss. Region residency is the catalogue's activation
//! bounds and nothing else: no boundary wait, no target machinery, no eviction.
//! The hero has nail, jump, walk and Focus, and none of the ability pickups.
//!
//! **A tape this approves is a candidate, not a result.** It has to be replayed
//! on the disc through `tools/replay_cue.py` before anyone believes it. What
//! this buys is that the replay is a confirmation rather than a search step.
//!
//! # Four things that silently break a tape
//!
//! Each of these was found by a tape that simulated clean and then failed on the
//! disc, and each costs an 85-second replay to rediscover. They are written down
//! here because they live in the guest, not in `hk-sim`, and nothing else in the
//! tree records why they matter.
//!
//! 1. **`Actor::local_bounds` mirrors the placement box.** `game/src/enemies.rs`
//!    returns `[-b[2], b[1], -b[0], b[3]]` once `walk.direction` leaves the
//!    cooked `initial_direction`, and `advance_false_knight` takes the body's
//!    collision offset from those bounds. For this placement that moves the
//!    solved body **0.142 world units** the moment the boss turns right, which
//!    is enough to decide whether a swing at the edge of nail reach lands.
//! 2. **`main.rs::respond_to_hurt` cancels the swing.** Any landed hit, from the
//!    body, from the `Hitter` trigger or from a barrel, resets `Nail` and
//!    `NailResponse` and interrupts the Focus. A model that lets the swing run
//!    through the hit invents nail hits across every recoil.
//! 3. **The barrel summoner is seeded from 1**, in `EnemyWorld::new`, not from
//!    the scene id. `clear_shots` re-seeds it with `scene as u32` on a scene
//!    load, and a route that never reloads the scene never reaches that. The
//!    seed decides both the 9-to-15-tick gaps and the spawn x of all eight
//!    barrels of a rage, so getting it wrong dodges the wrong barrels.
//! 4. **A tape sample reaches the simulation one tick after the pad was polled
//!    with it.** Tape index `N` is consumed on tick `N + delay()`. The guest also
//!    catches its simulation up in bursts, so `route.csv` can report a state one
//!    tick early or late at a given poll; that is log sampling, not a different
//!    simulation, which is why the drift test allows one poll of slack and no
//!    more.
//!
//! usage:
//!   boss_sim <world.txt> replay <tape.pxtape> [trace.csv] [summary.json]
//!   boss_sim <world.txt> run <masks.txt> [trace.csv] [summary.json]
//!   boss_sim <world.txt> search <prefix.txt> <segment> <horizon> <beam> <out.txt> <alphabet> [trace.csv]
extern crate hk_format;
extern crate hk_sim;

use hk_sim::false_knight as fk;
use hk_sim::{ActorHealth, EnemyParams, Hurt, Nail, NailResponse, Params, Player, Vitals, ONE};

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/params.rs"));

const B_UP: u16 = 16;
const B_RIGHT: u16 = 32;
const B_DOWN: u16 = 64;
const B_LEFT: u16 = 128;
const B_CIRCLE: u16 = 8192;
const B_CROSS: u16 = 16384;
const B_SQUARE: u16 = 32768;

/// `enemies.rs`: the shared projectile pool the barrels ride, and the
/// neighbourhood gate that decides whether an actor advances at all.
const SHOTS: usize = 8;
const NEAR: [i32; 2] = [24 * ONE, 16 * ONE];
/// Tape sample N reaches the simulation on tick N + delay(). Measured against a
/// replay, not assumed; see the fourth note above.
fn delay() -> usize {
    static DELAY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DELAY.get_or_init(|| std::env::var("BOSS_SIM_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(1))
}

/// The `Head`'s own HealthManager, as `enemies.rs`'s `FALSE_KNIGHT_HEAD`.
const FK_HEAD: EnemyParams = EnemyParams {
    health: fk::HEAD_HEALTH,
    contact_damage: 0,
    evasion_ticks: fk::HEAD_INVULNERABLE_TICKS,
    invincible: false,
    damage_override: false,
};

/// One catalogue slot of the boss's scene, as the wrapper read it out of
/// `data/regions.json` and `data/battle_gates.rs`.
struct RegionData {
    bounds: [i32; 4],
    collision: [i32; 4],
    edges: Vec<[i32; 4]>,
    /// The same view with the terrain of every gate the source loads open taken
    /// away, which is the state `battle_gates::apply` leaves it in until the
    /// arena's `BG CLOSE` puts the whole room's set back and seals the floor.
    open_edges: Vec<[i32; 4]>,
    /// The catalogue slot, which the wrapper's `floor` lines name.
    slot: i64,
    /// Both of the above with `Break Floor`'s colliders gone, which is what
    /// `battle_gates::apply` does once the death jump comes down through it.
    broken_edges: Vec<[i32; 4]>,
    broken_open_edges: Vec<[i32; 4]>,
}

/// The cooked `ActorSpec` of the boss, as the wrapper read it out of
/// `data/regions.rs`.
struct BossSpec {
    x: i32,
    y: i32,
    bounds: [i32; 4],
    body: EnemyParams,
    initial_direction: i32,
    trigger: [i32; 4],
    barrel_spawn_y: i32,
    seed: u32,
}

struct World {
    regions: Vec<RegionData>,
    spec: BossSpec,
    /// `Shockwave Wave`'s cooked numbers and its spawn height below the body.
    wave: hk_sim::shockwave::Params,
    wave_origin_y: i32,
    /// Where the hero stands once the card's save has been loaded and the title
    /// is gone. The simulator starts there rather than at the title, because
    /// nothing before it is gameplay; the wrapper carries the value and
    /// `tests/test_boss_sim.py` pins it against a replay's own `HK_PLAYER_X/Y`.
    start: [i32; 3],
    /// Search guard only: the x span of the sealed arena floor. A tape that
    /// leaves it has lost, and pruning it keeps the beam out of the region
    /// machinery this does not model.
    keep_x: [i32; 2],
}

fn contains(b: [i32; 4], x: i32, y: i32) -> bool {
    x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
/// `enemies.rs::segment_hits_terrain`.
fn segment_hits_terrain(a: [i32; 2], b: [i32; 2], edges: &[[i32; 4]]) -> bool {
    let (ox, oy) = (a[0] as i64, a[1] as i64);
    let (rx, ry) = (b[0] as i64 - ox, b[1] as i64 - oy);
    for e in edges {
        if *e == [0; 4] { continue; }
        let (ax, ay) = (e[0] as i64, e[1] as i64);
        let (sx, sy) = (e[2] as i64 - ax, e[3] as i64 - ay);
        let den = rx * sy - ry * sx;
        if den == 0 { continue; }
        let (qx, qy) = (ax - ox, ay - oy);
        let (mut t, mut u, mut d) = (qx * sy - qy * sx, qx * ry - qy * rx, den);
        if d < 0 { t = -t; u = -u; d = -d; }
        if t >= 0 && t <= d && u >= 0 && u <= d { return true; }
    }
    false
}
/// `enemies.rs::distance_q16`, `GetDistance` between two transforms.
fn distance_q16(dx: i32, dy: i32) -> i32 {
    let square = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
    if square <= 0 { return 0; }
    let mut x = 1i64 << ((64 - square.leading_zeros() as i64 + 1) / 2);
    loop {
        let next = (x + square / x) / 2;
        if next >= x { break; }
        x = next;
    }
    while x * x > square { x -= 1; }
    x as i32
}

/// A pooled `Falling Barrel`. The Aspid bullet half of `enemies.rs`'s `Shot` is
/// not modelled: Crossroads_10 is the only room with a boss and carries no Aspid.
#[derive(Clone, Copy)]
struct Shot { x: i32, y: i32, vy: i32, animation_tick: u32, live: bool }

#[derive(Clone, Copy)]
struct Sim {
    /// `game/src/input.rs` `hero_latency`: the previous tick's pad, what the last
    /// FixedUpdate-holding tick took of it, and the 50 Hz phase in sixths.
    lat_previous: u16,
    lat_taken: u16,
    lat_phase: u8,
    player: Player,
    response: NailResponse,
    nail: Nail,
    vitals: Vitals,
    focus: hk_sim::Focus,
    /// `Cast::held`, which is what gates the Focus behind `Button Down Time`.
    cast_held: u16,
    controller: fk::FalseKnight,
    arena: hk_sim::boss::Arena,
    head: ActorHealth,
    body: ActorHealth,
    bx: i32,
    by: i32,
    bvy: i32,
    bvx: i32,
    bgrounded: bool,
    gravity: i32,
    kinematic: bool,
    hitter: bool,
    contact_damage: u16,
    summon: u8,
    separated: bool,
    /// `WalkState::direction`, which is the sign `Actor::local_bounds` mirrors on.
    facing_dir: i32,
    summoner: fk::Summon,
    summon_armed: bool,
    shots: [Shot; SHOTS],
    /// `EnemyWorld::waves`, and the direction `S Attack Recover` asked for.
    waves: [Option<hk_sim::shockwave::Wave>; 2],
    wave_asked: Option<bool>,
    wave_hits: u32,
    waves_spawned: u32,
    active: usize,
    staggers: u32,
    conversions: u32,
    fk_deaths: u32,
    deaths: u32,
    hits: u32,
    entered: bool,
    left_arena: bool,
    gates_closed: bool,
    floor_broken: bool,
}

impl Sim {
    fn new(world: &World) -> Self {
        let spec = &world.spec;
        let mut player = Player::spawn(world.start[0], world.start[1]);
        player.facing = world.start[2];
        player.grounded = true;
        let active = world.regions.iter()
            .position(|r| contains(r.bounds, world.start[0], world.start[1]))
            .expect("the hero's start is inside a catalogue slot of the boss scene");
        Self {
            lat_previous: 0, lat_taken: 0, lat_phase: phase_at_load(),
            player,
            response: NailResponse::new(),
            nail: Nail::new(),
            vitals: Vitals::new(VITAL_PARAMS),
            focus: hk_sim::Focus::default(),
            cast_held: 0,
            // `Actor::new`'s stable per-source seed, in 32-bit arithmetic
            // because the guest's `usize` is 32 bits.
            controller: fk::FalseKnight::new(spec.seed, false),
            arena: hk_sim::boss::Arena::new(false),
            head: ActorHealth::new(FK_HEAD),
            body: ActorHealth::new(spec.body),
            bx: spec.x,
            by: spec.y,
            bvy: 0,
            bvx: 0,
            bgrounded: false,
            gravity: fk::GRAVITY_IDLE,
            kinematic: true,
            hitter: false,
            contact_damage: spec.body.contact_damage,
            summon: 0,
            separated: false,
            facing_dir: spec.initial_direction,
            // `EnemyWorld::new`'s seed, not the scene id; see the third note.
            summoner: fk::Summon::new(1),
            summon_armed: false,
            shots: [Shot { x: 0, y: 0, vy: 0, animation_tick: 0, live: false }; SHOTS],
            waves: [None; 2],
            wave_asked: None,
            wave_hits: 0,
            waves_spawned: 0,
            active,
            staggers: 0,
            conversions: 0,
            fk_deaths: 0,
            deaths: 0,
            hits: 0,
            entered: false,
            left_arena: false,
            gates_closed: false,
            floor_broken: false,
        }
    }
    /// `Actor::local_bounds`: the placement box mirrors once the walk direction
    /// leaves the cooked `initial_direction`. See the first note.
    fn local_bounds(&self, world: &World) -> [i32; 4] {
        let b = world.spec.bounds;
        if self.facing_dir == world.spec.initial_direction { b } else { [-b[2], b[1], -b[0], b[3]] }
    }
    fn boss_bounds(&self, world: &World) -> [i32; 4] {
        let b = self.local_bounds(world);
        [self.bx + b[0], self.by + b[1], self.bx + b[2], self.by + b[3]]
    }
    fn hitter_box(&self) -> [i32; 4] {
        let b = fk::HITTER_BOX;
        if self.controller.facing_right() {
            [self.bx + b[0], self.by + b[1], self.bx + b[2], self.by + b[3]]
        } else {
            [self.bx - b[2], self.by + b[1], self.bx - b[0], self.by + b[3]]
        }
    }
    fn head_box(&self) -> [i32; 4] {
        let b = fk::HEAD_BOX;
        [self.bx + b[0], self.by + b[1], self.bx + b[2], self.by + b[3]]
    }
    fn hero_body(&self) -> [i32; 4] {
        [self.player.x - PARAMS.half_width, self.player.y + PARAMS.bottom,
         self.player.x + PARAMS.half_width, self.player.y + PARAMS.top]
    }
    /// Damage taken off the pair of HealthManagers, counting a converted phase
    /// as the whole 105 it cost. This is the search's notion of progress, and it
    /// has to stay monotone across the death sequence: score the armour being
    /// open higher than the blow that empties it and the beam refuses to land
    /// the blow, which is exactly what it did before this read the phase.
    fn progress(&self) -> i32 {
        use fk::Phase::*;
        let extra = match self.controller.phase() {
            Dying(_) | Dead => 105,
            StunRoll | StunRollEnd | StunPause | StunOpening | Opened | StunHit
            | DeathJumpAntic | DeathAir | DeathHit | DeathFall | DeathLand | DeathOpening
            | DeathOpened | DeathHit2 => 65 + (40 - self.head.hp as i32),
            Recovering => 0,
            _ => 65 - self.body.hp as i32,
        };
        self.conversions as i32 * 105 + extra
    }
    fn locate(&self, world: &World, x: i32, y: i32) -> Option<usize> {
        world.regions.iter().position(|r| contains(r.bounds, x, y))
    }
    /// The view's terrain as `battle_gates::apply` leaves it.
    fn view<'a>(&self, region: &'a RegionData) -> &'a [[i32; 4]] {
        match (self.gates_closed, self.floor_broken) {
            (true, false) => &region.edges,
            (false, false) => &region.open_edges,
            (true, true) => &region.broken_edges,
            (false, true) => &region.broken_open_edges,
        }
    }

    /// One 60 Hz tick: `frame::simulate`'s hero path, then the False Knight
    /// branch of `EnemyWorld::tick`, in that order.
    fn step(&mut self, world: &World, raw: u16) {
        // The Knight's view of the pad: left, right and cross from the tick
        // before on the ticks that hold a 50 Hz step, from the last step's on
        // the one that holds none. Every tick passes through here, a frozen
        // Knight's too, as in the guest.
        self.lat_phase += 5;
        if self.lat_phase >= 6 { self.lat_phase -= 6; self.lat_taken = self.lat_previous; }
        let m = (raw & !LATE) | (self.lat_taken & LATE);
        self.lat_previous = raw;
        let held = |b: u16| m & b != 0;
        if self.vitals.tick() { return; }
        if self.vitals.needs_death_respawn() { self.deaths += 1; return; }

        let circle = held(B_CIRCLE);
        if circle { self.cast_held = self.cast_held.saturating_add(1); } else { self.cast_held = 0; }
        let focus_events = self.focus.step(
            FOCUS_PARAMS, VITAL_PARAMS,
            hk_sim::FocusInput {
                held: circle && self.cast_held > FIREBALL_PARAMS.tap_ticks,
                grounded: self.player.grounded,
                can_start: self.vitals.can_control()
                    && (!self.nail.active || self.nail.age >= FOCUS_PARAMS.attack_recovery_ticks),
            },
            &mut self.vitals,
        );
        if focus_events.started {
            self.nail = Nail::new();
            self.response = NailResponse::new();
            self.player.vy = 0;
            self.player.jumping = false;
        }
        let locked = self.focus.locks_control();
        let dir = if locked { 0 } else { i32::from(held(B_RIGHT)) - i32::from(held(B_LEFT)) };
        let jump = !locked && held(B_CROSS);

        let hero_edges = self.view(&world.regions[self.active]);
        if let Some((vx, vy)) = self.vitals.recoil_velocity(VITAL_PARAMS) {
            let mut p = PARAMS;
            p.speed = vx.abs();
            p.gravity = 0;
            self.player.vy = vy;
            self.player.jumping = false;
            self.player.step(p, vx.signum(), false, hero_edges.len(), |i| hero_edges[i]);
            self.player.facing = -self.vitals.recoil_direction;
        } else {
            self.response.step(NAIL_RESPONSE_PARAMS, PARAMS, &mut self.player, dir, jump,
                hero_edges.len(), |i| hero_edges[i]);
        }
        let vertical = i32::from(held(B_UP)) - i32::from(held(B_DOWN));
        let button = held(B_SQUARE) && self.vitals.can_control() && !locked;
        self.nail.tick(ATTACK_PARAMS, button, vertical, &mut self.player);

        let mut points = [[0i32; 2]; 16];
        let polygon: &[[i32; 2]] = if self.nail.hitting(ATTACK_PARAMS) {
            let src = NAIL_POLYGONS[self.nail.kind as usize];
            for (dst, p) in points.iter_mut().zip(src) {
                *dst = [self.player.x - p[0] * self.player.facing, self.player.y + p[1]];
            }
            &points[..src.len()]
        } else { &points[..0] };
        let hero_body = self.hero_body();

        if !self.body.dead { self.body.tick(); }
        let ab = world.regions[self.active].bounds;
        // `enemies.rs` lets the boss fall out of that range after the floor breaks.
        let near = contains([ab[0] - NEAR[0], ab[1] - NEAR[1], ab[2] + NEAR[0], ab[3] + NEAR[1]],
            self.bx, self.by) || self.controller.phase() == fk::Phase::DeathFall;
        let target = if contains(ab, self.bx, self.by) { self.active }
            else { self.locate(world, self.bx, self.by).unwrap_or(self.active) };
        let available = near && contains(world.regions[target].collision, self.bx, self.by);
        if self.body.dead {
            // `advance_body`'s dead path keeps `End Wait` counting.
            let _ = self.arena.tick();
        } else if available {
            let edges = self.view(&world.regions[target]);
            self.advance_boss(world, edges, hero_body);
        }
        let mut hurt = Hurt::Ignored;
        if !self.body.dead && self.controller.phase() != fk::Phase::Dormant {
            let mut reached = false;
            if !polygon.is_empty() {
                let (hit_box, hit) = self.strike(world,
                    |b| hk_sim::polygon_hits_box(polygon, b), VITAL_PARAMS.nail_damage);
                reached = hit_box;
                if matches!(hit, hk_sim::Hit::Damaged | hk_sim::Hit::Killed) {
                    self.hits += 1;
                    self.vitals.gain_soul_on_nail_hit(VITAL_PARAMS);
                }
            }
            if reached && self.vitals.can_control() {
                let (kind, facing) = (self.nail.kind, self.player.facing);
                self.response.contact(NAIL_RESPONSE_PARAMS, kind, facing, &mut self.player);
            }
            let mut damage = 0;
            if self.contact_damage != 0 && overlap(hero_body, self.boss_bounds(world)) {
                damage = self.contact_damage;
            }
            if self.hitter && overlap(hero_body, self.hitter_box()) { damage = fk::CONTACT_DAMAGE; }
            if damage != 0 {
                let direction = if self.player.x < self.bx { -1 } else { 1 };
                let outcome = self.vitals.hurt(VITAL_PARAMS, damage, direction, false);
                if outcome != Hurt::Ignored { hurt = outcome; }
            }
        }
        // `EnemyWorld::spawn_wave`, ahead of the summoner as the guest drains it.
        if let Some(right) = self.wave_asked.take() {
            let dir = if right { 1 } else { -1 };
            let wave = hk_sim::shockwave::Wave::new(
                [self.bx + dir * fk::SHOCKWAVE_X_ORIGIN, self.by + world.wave_origin_y], dir, &world.wave);
            let slot = self.waves.iter().position(Option::is_none).unwrap_or_else(|| {
                let mut oldest = 0;
                for (i, w) in self.waves.iter().enumerate() {
                    if w.map_or(0, |w| w.age()) >= self.waves[oldest].map_or(0, |w| w.age()) { oldest = i; }
                }
                oldest
            });
            self.waves[slot] = Some(wave);
            self.waves_spawned += 1;
        }
        if self.summon != 0 {
            let spawns = core::mem::take(&mut self.summon);
            self.summon_armed = true;
            self.summoner.summon(spawns);
        }
        if self.summon_armed {
            if let Some(x) = self.summoner.tick() {
                // `EnemyWorld::spawn_projectile`: the oldest live slot is
                // recycled when the pool is full, which is the source's rule.
                let slot = self.shots.iter().position(|s| !s.live).unwrap_or_else(|| {
                    let mut oldest = 0;
                    for i in 0..SHOTS {
                        if self.shots[i].animation_tick >= self.shots[oldest].animation_tick {
                            oldest = i;
                        }
                    }
                    oldest
                });
                self.shots[slot] =
                    Shot { x, y: world.spec.barrel_spawn_y, vy: 0, animation_tick: 0, live: true };
            }
        }
        let outcome = self.tick_shots(world, hero_body);
        if outcome != Hurt::Ignored { hurt = outcome; }
        // `EnemyWorld::tick_waves`.
        let edges = self.view(&world.regions[self.active]).to_vec();
        for slot in self.waves.iter_mut() {
            let Some(wave) = slot else { continue };
            let done = wave.step(&world.wave, |a, b| segment_hits_terrain(a, b, &edges));
            if wave.hurts(&world.wave, hero_body) {
                let outcome = self.vitals.hurt(VITAL_PARAMS, world.wave.damage, wave.dir as i32, false);
                if outcome != Hurt::Ignored { hurt = outcome; self.wave_hits += 1; }
            }
            if done { *slot = None; }
        }
        // `main.rs::respond_to_hurt`; see the second note.
        if hurt != Hurt::Ignored {
            self.nail = Nail::new();
            self.response = NailResponse::new();
            self.focus.interrupt();
        }
        if let Some(id) = self.locate(world, self.player.x, self.player.y) { self.active = id; }
        if self.player.x >= world.keep_x[0] && self.player.x <= world.keep_x[1] { self.entered = true; }
        else if self.entered { self.left_arena = true; }
    }

    /// `Actor::strike_false_knight`.
    fn strike(&mut self, world: &World, hits: impl Fn([i32; 4]) -> bool, damage: u16)
        -> (bool, hk_sim::Hit) {
        if self.controller.head_exposed() {
            if !hits(self.head_box()) { return (false, hk_sim::Hit::Ignored); }
            let hit = self.head.hit(FK_HEAD, damage);
            let actions = if self.head.dead {
                self.head = ActorHealth::new(FK_HEAD);
                self.controller.head_reached_zero()
            } else if hit == hk_sim::Hit::Damaged {
                self.controller.head_hit()
            } else {
                return (true, hit);
            };
            self.apply(actions);
            return (true, hk_sim::Hit::Damaged);
        }
        if !hits(self.boss_bounds(world)) { return (false, hk_sim::Hit::Ignored); }
        if self.controller.invincible() { return (true, hk_sim::Hit::Blocked); }
        let hit = self.body.hit(world.spec.body, damage);
        if !self.body.dead { return (true, hit); }
        self.body = ActorHealth::new(world.spec.body);
        let actions = self.controller.body_reached_zero();
        if !actions.is_empty() { self.staggers += 1; }
        self.apply(actions);
        (true, hk_sim::Hit::Damaged)
    }

    /// `Actor::apply_false_knight`, minus the clips, the shakes and the audio.
    fn apply(&mut self, actions: fk::Actions) {
        use fk::Action;
        for action in actions.iter() {
            match action {
                Action::Play(_) | Action::PlayHitter(_) => {}
                Action::Velocity([x, y]) => { self.bvx = x; self.bvy = y; self.bgrounded = false; }
                Action::VelocityX(x) => self.bvx = x,
                Action::Gravity(g) => self.gravity = g,
                Action::Facing(f) => self.facing_dir = -f,
                Action::Hitter(on) => self.hitter = on,
                Action::ContactDamage(d) => self.contact_damage = d,
                Action::Kinematic(k) => {
                    self.kinematic = k;
                    if k { self.bvy = 0; self.bvx = 0; }
                }
                Action::KillAllEnemies => { let _ = self.arena.kill_all_enemies(); }
                Action::Died => {
                    self.arena.enemy_died();
                    self.body.dead = true;
                    self.fk_deaths += 1;
                }
                Action::Staggered(_) => self.conversions += 1,
                Action::SummonBarrels(s) => self.summon = s,
                // `DESTROY`: `Break Floor`'s terrain goes, under the boss and
                // under a hero standing on it alike.
                Action::BreakFloor => self.floor_broken = true,
                Action::Shockwave { right, .. } => self.wave_asked = Some(right),
                // Art, shakes and voices.
                Action::SetFirstPlop | Action::Effect(_) | Action::Invincible(_)
                | Action::HeadExposed(_) | Action::CrackFloor
                | Action::Head(_) | Action::Blow | Action::DeathHeadLand => {}
            }
        }
    }

    /// `Actor::advance_false_knight`.
    fn advance_boss(&mut self, world: &World, edges: &[[i32; 4]], hero_body: [i32; 4]) {
        if self.controller.phase() == fk::Phase::Dormant {
            if overlap(world.spec.trigger, hero_body) {
                let started = self.arena.hero_entered();
                if started.contains(hk_sim::boss::Action::CloseGates) { self.gates_closed = true; }
                if started.contains(hk_sim::boss::Action::StartBattle) {
                    let a = self.controller.battle_start();
                    self.apply(a);
                }
            }
            return;
        }
        let _ = self.arena.tick();
        self.head.tick();
        let b = self.local_bounds(world);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params {
            speed: self.bvx.abs(),
            gravity: (((60 * ONE) as i64 * self.gravity as i64) >> 16) as i32,
            fall: 100 * ONE,
            half_width: (b[2] - b[0]) / 2,
            bottom: b[1],
            top: b[3],
            ..Params::ZERO
        };
        let mut body = Player::spawn(self.bx + offset, self.by);
        body.vy = self.bvy;
        body.grounded = self.bgrounded;
        if !self.separated {
            // `Start Fall` turns gravity on while the body is still inside the
            // ceiling slab it is authored in.
            if !hk_sim::resolve_actor_spawn(&mut body, p, edges.len(), |i| edges[i]) { return; }
            self.separated = true;
        }
        if !self.kinematic {
            body.step(p, self.bvx.signum(), false, edges.len(), |i| edges[i]);
        }
        self.bx = body.x - offset;
        self.by = body.y;
        self.bvy = body.vy;
        self.bgrounded = body.grounded;
        let phase = self.controller.phase();
        // Only the states that read a sense pay for it, as the guest does.
        let idle = phase == fk::Phase::Idle;
        let falling = matches!(phase, fk::Phase::JumpAttackAir | fk::Phase::DeathAir);
        let distance = if idle || phase == fk::Phase::Run {
            distance_q16(self.player.x - self.bx, self.player.y - self.by)
        } else { 0 };
        let senses = fk::Senses {
            self_x: self.bx,
            hero_x: self.player.x,
            distance,
            velocity_y: body.vy,
            velocity_x: self.bvx,
            grounded: body.grounded,
            wall_left: idle && segment_hits_terrain([self.bx, self.by],
                [self.bx - fk::WALL_RAY_DISTANCE, self.by], edges),
            wall_right: idle && segment_hits_terrain([self.bx, self.by],
                [self.bx + fk::WALL_RAY_DISTANCE, self.by], edges),
            ground_below: falling && segment_hits_terrain([self.bx, self.by],
                [self.bx, self.by - fk::FALL_RAY_DISTANCE], edges),
        };
        let actions = self.controller.tick(senses);
        self.apply(actions);
    }

    /// `EnemyWorld::tick_shots`, barrels only.
    fn tick_shots(&mut self, world: &World, hero_body: [i32; 4]) -> Hurt {
        let mut result = Hurt::Ignored;
        let region = &world.regions[self.active];
        for i in 0..SHOTS {
            if !self.shots[i].live { continue; }
            let mut shot = self.shots[i];
            shot.animation_tick = shot.animation_tick.saturating_add(1);
            shot.vy -= fk::BARREL_GRAVITY / 60;
            let (ox, oy) = (shot.x, shot.y);
            shot.y += shot.vy / 60;
            if !contains(region.collision, shot.x, shot.y)
                && !contains(region.bounds, shot.x, shot.y)
                && (shot.x.abs() > 512 * ONE || shot.y.abs() > 512 * ONE)
            {
                self.shots[i].live = false;
                continue;
            }
            let hit_terrain = segment_hits_terrain([ox, oy], [shot.x, shot.y], self.view(region));
            let half = fk::BARREL_HALF;
            let bounds = [shot.x - half[0], shot.y - half[1], shot.x + half[0], shot.y + half[1]];
            let hit_hero = overlap(bounds, hero_body);
            if hit_hero {
                // A barrel falls straight down, so the recoil side is the side
                // of the hero it landed on.
                let dir = if shot.x < (hero_body[0] + hero_body[2]) / 2 { -1 } else { 1 };
                let outcome = self.vitals.hurt(VITAL_PARAMS, fk::BARREL_DAMAGE, dir, false);
                if outcome != Hurt::Ignored { result = outcome; }
            }
            // `Break` turns the sprite off, so a broken barrel leaves at once.
            if hit_terrain || hit_hero { self.shots[i].live = false; } else { self.shots[i] = shot; }
        }
        result
    }
}

fn field(line: &str) -> Vec<i64> {
    line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect()
}

/// Read the world the wrapper wrote: one `slot`/`actor`/`keep_x` directive a
/// line, whitespace separated, a room pack path last on a `slot` line.
fn load_world(path: &str) -> World {
    let text = std::fs::read_to_string(path).unwrap();
    let mut regions: Vec<RegionData> = Vec::new();
    let mut spec: Option<BossSpec> = None;
    let mut keep_x = [i32::MIN, i32::MAX];
    let mut start = [0i32; 3];
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    let mut wave = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let kind = line.split_whitespace().next().unwrap();
        match kind {
            "wave" => {
                let v: Vec<i32> = field(line).iter().map(|&n| n as i32).collect();
                wave = Some((v[0], hk_sim::shockwave::Params {
                    start_speed: v[1], accel: v[2], wave_box: [v[3], v[4], v[5], v[6]], ground_ray: v[7],
                    spurt_box: [v[8], v[9], v[10], v[11]], damage_from: v[12] as u16, damage_to: v[13] as u16,
                    damage: v[14] as u16, spurt_ticks: v[15] as u16,
                }));
            }
            "keep_x" => {
                let v = field(line);
                keep_x = [v[0] as i32, v[1] as i32];
            }
            "start" => {
                let v = field(line);
                start = [v[0] as i32, v[1] as i32, v[2] as i32];
            }
            "actor" => {
                let v = field(line);
                spec = Some(BossSpec {
                    x: v[0] as i32,
                    y: v[1] as i32,
                    bounds: [v[2] as i32, v[3] as i32, v[4] as i32, v[5] as i32],
                    body: EnemyParams {
                        health: v[6] as i16,
                        contact_damage: v[7] as u16,
                        evasion_ticks: v[8] as u16,
                        invincible: v[9] != 0,
                        damage_override: v[10] != 0,
                    },
                    initial_direction: v[11] as i32,
                    trigger: [v[12] as i32, v[13] as i32, v[14] as i32, v[15] as i32],
                    barrel_spawn_y: v[16] as i32,
                    seed: v[17] as u32,
                });
            }
            "slot" => {
                let mut parts = line.split_whitespace();
                parts.next();
                let numbers: Vec<i64> = parts.clone().filter_map(|v| v.parse().ok()).collect();
                // The pack path is last: repo-relative as tools/boss_sim.py
                // writes it, or absolute and possibly holding spaces (a checkout
                // under "Application Support") as an older wrapper wrote it.
                let pack = line.find(" /").map(|i| &line[i + 1..])
                    .unwrap_or_else(|| line.split_whitespace().last().unwrap());
                blobs.push(std::fs::read(pack).unwrap());
                let room = hk_format::Room::parse(blobs.last().unwrap()).unwrap();
                let edges: Vec<[i32; 4]> = (0..room.counts[5]).map(|n| room.edge(n)).collect();
                let mut open_edges = edges.clone();
                for n in numbers[9..].iter() {
                    if (*n as usize) < open_edges.len() { open_edges[*n as usize] = [0; 4]; }
                }
                regions.push(RegionData {
                    bounds: [numbers[1] as i32, numbers[2] as i32, numbers[3] as i32, numbers[4] as i32],
                    collision: [numbers[5] as i32, numbers[6] as i32, numbers[7] as i32, numbers[8] as i32],
                    broken_edges: edges.clone(),
                    broken_open_edges: open_edges.clone(),
                    edges,
                    open_edges,
                    slot: numbers[0],
                });
            }
            "floor" => {
                let v = field(line);
                let region = regions.iter_mut().find(|r| r.slot == v[0])
                    .expect("a floor line names a slot the world carries");
                for n in &v[1..] {
                    let n = *n as usize;
                    if n < region.edges.len() {
                        region.broken_edges[n] = [0; 4];
                        region.broken_open_edges[n] = [0; 4];
                    }
                }
            }
            other => panic!("unknown world directive {other}"),
        }
    }
    // `blobs` is dropped here, so the parsed edge tables must already own their
    // values; they do, because `Room::edge` returns a decoded array.
    let (wave_origin_y, wave) = wave.expect("world file carries the slam wave");
    World { regions, spec: spec.expect("world file carries the boss ActorSpec"), start, keep_x, wave, wave_origin_y }
}

/// Left, right and cross, which the Knight's simulation reads a tick late, as
/// the original does (`game/src/input.rs` `hero_latency`).
const LATE: u16 = (1 << 7) | (1 << 5) | (1 << 14);

/// A tape's samples as polled. The latency (`Sim::step`) is the simulation's.
fn read_tape(path: &str) -> Vec<u16> {
    let tape = std::fs::read(path).unwrap();
    assert_eq!(&tape[..8], b"PXITAPE2", "not a poll-bound tape");
    let count = u32::from_le_bytes(tape[8..12].try_into().unwrap()) as usize;
    (0..count)
        .map(|n| u16::from_le_bytes(tape[16 + n * 6..18 + n * 6].try_into().unwrap()))
        .collect()
}
/// Where the 50 Hz phase stands before the first simulated tick: the disc's
/// `PHASE_AT_LOAD` plus the ticks of load the simulation does not model. Found
/// by replaying a tape and matching the Knight's trace (phase 4 for a disc at
/// phase 0, the default; add the disc's phase), overridden by `BOSS_SIM_PHASE`.
fn phase_at_load() -> u8 {
    std::env::var("BOSS_SIM_PHASE").ok().and_then(|v| v.parse::<u8>().ok()).unwrap_or(4) % 6
}

const HEADER: &str = "poll,hx,hy,grounded,health,soul,fkx,fky,fkhp,headhp,phase,stunned,exposed,staggers,conversions,deaths,barrels,facing\n";

fn trace_line(out: &mut String, poll: usize, s: &Sim) {
    use std::fmt::Write;
    let _ = writeln!(
        out,
        "{},{:.4},{:.4},{},{},{},{:.4},{:.4},{},{},{:?},{},{},{},{},{},{},{}",
        poll, s.player.x as f64 / ONE as f64, s.player.y as f64 / ONE as f64,
        u8::from(s.player.grounded), s.vitals.health, s.vitals.soul,
        s.bx as f64 / ONE as f64, s.by as f64 / ONE as f64, s.body.hp, s.head.hp,
        s.controller.phase(), s.controller.stunned(), u8::from(s.controller.head_exposed()),
        s.staggers, s.conversions, s.fk_deaths,
        s.shots.iter().filter(|x| x.live).count(), s.player.facing,
    );
}

/// The three counters a replay can see change, as `[poll, from, to]` triples.
/// `tests/test_boss_sim.py` compares these against a captured `route.csv`,
/// which is the whole of the tick-exactness claim.
#[derive(Default)]
struct Transitions { fk_hp: Vec<(usize, i32, i32)>, head_hp: Vec<(usize, i32, i32)>, health: Vec<(usize, i32, i32)> }

fn replay(world: &World, masks: &[u16], trace: Option<&str>, summary: Option<&str>) -> Sim {
    let mut s = Sim::new(world);
    let mut out = String::from(HEADER);
    let mut t = Transitions::default();
    let (mut fk_hp, mut head_hp, mut health) = (s.body.hp as i32, s.head.hp as i32, s.vitals.health as i32);
    for poll in 0..masks.len() + delay() {
        let m = if poll >= delay() && poll - delay() < masks.len() { masks[poll - delay()] } else { 0 };
        s.step(world, m);
        if trace.is_some() { trace_line(&mut out, poll, &s); }
        if s.body.hp as i32 != fk_hp { t.fk_hp.push((poll, fk_hp, s.body.hp as i32)); fk_hp = s.body.hp as i32; }
        if s.head.hp as i32 != head_hp { t.head_hp.push((poll, head_hp, s.head.hp as i32)); head_hp = s.head.hp as i32; }
        if s.vitals.health as i32 != health { t.health.push((poll, health, s.vitals.health as i32)); health = s.vitals.health as i32; }
        if s.deaths != 0 { break; }
    }
    if let Some(path) = trace { std::fs::write(path, out).unwrap(); }
    if let Some(path) = summary { std::fs::write(path, summary_json(&s, &t, masks.len())).unwrap(); }
    s
}

fn triples(name: &str, rows: &[(usize, i32, i32)]) -> String {
    let body: Vec<String> = rows.iter().map(|(p, a, b)| format!("[{p},{a},{b}]")).collect();
    format!("\"{name}\": [{}]", body.join(", "))
}

fn summary_json(s: &Sim, t: &Transitions, polls: usize) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"polls\": {polls},\n"));
    out.push_str("  \"final\": {\n");
    out.push_str(&format!("    \"health\": {},\n", s.vitals.health));
    out.push_str(&format!("    \"soul\": {},\n", s.vitals.soul));
    out.push_str(&format!("    \"hero_deaths\": {},\n", s.deaths));
    out.push_str(&format!("    \"fk_hp\": {},\n", s.body.hp));
    out.push_str(&format!("    \"head_hp\": {},\n", s.head.hp));
    out.push_str(&format!("    \"staggers\": {},\n", s.staggers));
    out.push_str(&format!("    \"conversions\": {},\n", s.conversions));
    out.push_str(&format!("    \"fk_deaths\": {},\n", s.fk_deaths));
    out.push_str(&format!("    \"arena\": \"{:?}\",\n", s.arena.phase()));
    out.push_str(&format!("    \"phase\": \"{:?}\",\n", s.controller.phase()));
    out.push_str(&format!("    \"waves\": {},\n", s.waves_spawned));
    out.push_str(&format!("    \"wave_hits\": {},\n", s.wave_hits));
    out.push_str(&format!("    \"left_arena\": {}\n", s.left_arena));
    out.push_str("  },\n  \"transitions\": {\n");
    out.push_str(&format!("    {},\n", triples("fk_hp", &t.fk_hp)));
    out.push_str(&format!("    {},\n", triples("head_hp", &t.head_hp)));
    out.push_str(&format!("    {}\n", triples("health", &t.health)));
    out.push_str("  }\n}\n");
    out
}

fn report(tag: &str, s: &Sim, polls: usize) {
    println!(
        "{tag}: polls={polls} health={} soul={} hero_deaths={} fk_hp={} head_hp={} staggers={} \
         conversions={} fk_deaths={} arena={:?} phase={:?} progress={} hero=({:.2},{:.2}) \
         boss=({:.2},{:.2}) nail_hits={} left_arena={}",
        s.vitals.health, s.vitals.soul, s.deaths, s.body.hp, s.head.hp, s.staggers,
        s.conversions, s.fk_deaths, s.arena.phase(), s.controller.phase(), s.progress(),
        s.player.x as f64 / ONE as f64, s.player.y as f64 / ONE as f64,
        s.bx as f64 / ONE as f64, s.by as f64 / ONE as f64, s.hits, s.left_arena,
    );
}

fn score(s: &Sim, world: &World) -> i64 {
    let mut v = s.progress() as i64 * 1000;
    v += s.vitals.health as i64 * 22_000;
    v += s.vitals.soul as i64 * 40;
    v += s.fk_deaths as i64 * 400_000;
    if s.arena.phase() == hk_sim::boss::Phase::Open { v += 200_000; }
    // Shaping, so the beam can see a swing it has not made yet. The nail is a
    // five-tick window inside a jump, which no one-segment lookahead finds by
    // accident: pay for the gap between where the swing would reach and the box
    // it has to reach, and the beam climbs toward the pose instead.
    v -= reach_gap(s, world).min(8 * ONE as i64) * 1400 / ONE as i64;
    v
}

/// World distance from where the current swing's nail would land to the box the
/// nail has to reach: the exposed Head, or the body while the armour is shut.
fn reach_gap(s: &Sim, world: &World) -> i64 {
    if s.controller.phase() == fk::Phase::Dormant { return 0; }
    let target = if s.controller.head_exposed() { s.head_box() } else { s.boss_bounds(world) };
    // A horizontal swing reaches 3.28 units ahead, 0.81 above and 1.47 below the
    // transform: `data/params.rs`'s first nail polygon.
    const REACH: i32 = 214901;
    const TOP: i32 = 53067;
    const BOTTOM: i32 = -96083;
    let (ax, ay) = (s.player.x, s.player.y);
    let dx = if ax < target[0] - REACH { target[0] - REACH - ax }
        else if ax > target[2] + REACH { ax - target[2] - REACH } else { 0 };
    let dy = if ay + TOP < target[1] { target[1] - ay - TOP }
        else if ay + BOTTOM > target[3] { ay + BOTTOM - target[3] } else { 0 };
    dx as i64 + dy as i64
}

struct Node { sim: Sim, hist: i32, score: i64 }

/// Beam search over fixed-length input segments. The chosen masks live in a
/// parent-linked arena rather than a `Vec` per node, because copying a growing
/// mask list per candidate is what makes a beam this wide unaffordable.
fn search(world: &World, prefix: &[u16], segment: usize, horizon: usize, beam: usize,
          alphabet: &[u16]) -> (Vec<u16>, Sim) {
    let mut seed = Sim::new(world);
    for poll in 0..prefix.len() {
        let m = if poll >= delay() { prefix[poll - delay()] } else { 0 };
        seed.step(world, m);
    }
    let mut history: Vec<(i32, u16)> = Vec::new();
    let start = score(&seed, world);
    let mut nodes = vec![Node { sim: seed, hist: -1, score: start }];
    let total = horizon.saturating_sub(prefix.len()) / segment;
    let mut finished: Option<Node> = None;
    for step in 0..total {
        let mut next: Vec<Node> = Vec::with_capacity(nodes.len() * alphabet.len());
        for node in &nodes {
            for &m in alphabet {
                let mut sim = node.sim;
                for _ in 0..segment { sim.step(world, m); }
                if sim.deaths != 0 || sim.left_arena { continue; }
                history.push((node.hist, m));
                let hist = history.len() as i32 - 1;
                let sc = score(&sim, world);
                next.push(Node { sim, hist, score: sc });
            }
        }
        if next.is_empty() {
            eprintln!("search collapsed at step {step}");
            break;
        }
        next.sort_by_key(|n| -n.score);
        // Keep the beam diverse: a handful of survivors per (progress, health,
        // half-unit of hero x) bucket, so one line cannot crowd out the rest.
        let mut kept: Vec<Node> = Vec::with_capacity(beam);
        let mut buckets: std::collections::HashMap<(i32, u16, i32), usize> =
            std::collections::HashMap::new();
        for n in next.into_iter() {
            let key = (n.sim.progress(), n.sim.vitals.health, n.sim.player.x / (ONE / 2));
            let seen = buckets.entry(key).or_insert(0);
            if *seen >= 3 { continue; }
            *seen += 1;
            kept.push(n);
            if kept.len() >= beam { break; }
        }
        nodes = kept;
        if step % 25 == 0 {
            let b = &nodes[0];
            eprintln!("step {step} poll {} score {} progress {} health {} conv {} phase {:?}",
                prefix.len() + (step + 1) * segment, b.score, b.sim.progress(),
                b.sim.vitals.health, b.sim.conversions, b.sim.controller.phase());
        }
        if let Some(done) = nodes.iter().find(|n| n.sim.arena.phase() == hk_sim::boss::Phase::Open) {
            eprintln!("arena opened at poll {}", prefix.len() + (step + 1) * segment);
            finished = Some(Node { sim: done.sim, hist: done.hist, score: done.score });
            break;
        }
    }
    let best = finished.unwrap_or_else(|| nodes.into_iter().max_by_key(|n| n.score).unwrap());
    let mut tail: Vec<u16> = Vec::new();
    let mut cursor = best.hist;
    while cursor >= 0 {
        let (parent, m) = history[cursor as usize];
        for _ in 0..segment { tail.push(m); }
        cursor = parent;
    }
    tail.reverse();
    // The seed consumed tape samples 0 .. prefix.len()-delay(), because a sample
    // reaches the simulation delay() ticks after the pad was polled with it. The
    // search's first action therefore belongs at that index, not at prefix.len().
    let mut masks = prefix[..prefix.len() - delay()].to_vec();
    masks.extend(tail);
    (masks, best.sim)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let world = load_world(&args[1]);
    assert!(world.regions.len() > 1,
        "the world file carries {} catalogue slots; the boss scene has twenty",
        world.regions.len());
    match args[2].as_str() {
        "replay" | "run" => {
            let masks = if args[2] == "replay" {
                read_tape(&args[3])
            } else {
                std::fs::read_to_string(&args[3]).unwrap()
                    .split_whitespace().map(|v| v.parse().unwrap()).collect()
            };
            let trace = args.get(4).filter(|s| s.as_str() != "-").map(|s| s.as_str());
            let summary = args.get(5).filter(|s| s.as_str() != "-").map(|s| s.as_str());
            let s = replay(&world, &masks, trace, summary);
            report(&args[2], &s, masks.len());
        }
        "search" => {
            let text = std::fs::read_to_string(&args[3]).unwrap();
            let prefix: Vec<u16> = text.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let segment: usize = args[4].parse().unwrap();
            let horizon: usize = args[5].parse().unwrap();
            let beam: usize = args[6].parse().unwrap();
            let out = &args[7];
            let alphabet: Vec<u16> = args[8].split(',').map(|v| v.parse().unwrap()).collect();
            let (masks, s) = search(&world, &prefix, segment, horizon, beam, &alphabet);
            report("search", &s, masks.len());
            std::fs::write(out, masks.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(" "))
                .unwrap();
            if let Some(trace) = args.get(9).filter(|s| s.as_str() != "-") {
                replay(&world, &masks, Some(trace), args.get(10).map(|s| s.as_str()));
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
