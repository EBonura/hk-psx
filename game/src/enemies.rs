//! Source WalkLeftRight / HealthManager subset. States survive cooked grid swaps.
//! Actors use their own resident collision view, independently of the camera.
//! Actors outside all loaded views are suspended. Full Unity activation, Box2D
//! solver parity and global FSM events remain outside this bounded runtime.
use crate::world::{Region, State};
use hk_format::{i32_at, u32_at, Room};
use hk_sim::{
    ActorController, ActorHealth, ActorSpec, AttackParams, Hit, Hurt, Nail, Params, Player, Vitals, WalkState,
    MAX_ACTORS, ONE,
};

/// Source AudioSource/Charge Dust actions. The caller must own these events even
/// before a bank/effect is bound; never silently discard them for enabled actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerEventKind {
    AudioStop,
    AudioPlay,
    ChaseSound { pitch_q16: i32, variant: u8 },
    DustStart,
    DustStop,
    /// GameObject lifetime cleanup, not a fabricated Swipe FSM transition.
    Destroy,
    /// Aspid FireAtTarget: spawn a shot at the actor with this velocity.
    Fire { velocity: [i32; 2], shot_clip: u16, impact_clip: u16 },
    /// `SUMMON` to `FK Barrel Summon`, with `Spawns` already written. The
    /// summoner is a different source object from the boss and keeps its own
    /// clock, so this only hands over the count and where the barrels come
    /// from; `EnemyWorld` owns the loop that spaces them out.
    Summon { spawns: u8, spawn_y: i32, barrel_clip: u16 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerEvent {
    pub scene: usize,
    pub source_id: u32,
    pub position: [i32; 2],
    pub facing: i32,
    pub kind: RunnerEventKind,
}
#[derive(Clone, Copy)]
struct RunnerRuntime {
    controller: hk_sim::runner::Runner,
    rng: u32,
    vx: i32,
    animation_tick: u32,
}
impl RunnerRuntime {
    fn draw(rng: &mut u32) -> u32 {
        *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        *rng
    }
    fn choose(rng: &mut u32, _wait: hk_sim::runner::Wait, [hi, lo]: [u16; 2]) -> u16 {
        lo + (Self::draw(rng) % (hi - lo + 1) as u32) as u16
    }
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Runner { idle_clip, anticipate_clip, lunge_clip, cooldown_clip, .. } = spec.controller
            else { panic!("Runner runtime with Crawler metadata") };
        use hk_sim::runner::Clip;
        match self.controller.animation().map(|a| a.clip).unwrap_or(Clip::Idle) {
            Clip::Idle => idle_clip,
            Clip::Walk => spec.walk_clip,
            Clip::Turn => spec.turn_clip,
            Clip::Anticipate => anticipate_clip,
            Clip::Lunge => lunge_clip,
            Clip::Cooldown => cooldown_clip,
        }
    }
    fn completed(&self, spec: &ActorSpec, room: &Room) -> Option<hk_sim::runner::Animation> {
        let token = self.controller.animation()?;
        let id = self.clip(spec) as usize;
        assert!(id < room.counts[4], "Runner clip not resident");
        let clip = room.clip(id);
        assert!(clip[1] != 0 && clip[2] != 0, "empty Runner clip");
        // Once clips complete at their actual cooked frames/fps boundary.
        // Loop/LoopSection clips never manufacture a completion callback.
        if clip[3] & 65535 == 2
            && self.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536 {
            Some(token)
        } else { None }
    }
}
#[derive(Clone, Copy)]
struct RunnerContext {
    camera: [i32; 3],
    hero: [i32; 2],
    hero_body: [i32; 4],
}

/// The False Knight fight: `hk_sim::false_knight::FalseKnight` is `FalseyControl`
/// and `hk_sim::boss::Arena` is the `Battle Scene`'s `Battle Control`. The arena
/// rides on the boss rather than on a world object because the boss is the only
/// thing in the room it drives, and because the source's own counter never moves
/// except from the boss's `Decrement Battle Enemies`.
///
/// The body hangs kinematic above the arena in `Dormant` until the hero crosses
/// the trigger box, so before the fight this costs one AABB a tick and no
/// solver, no terrain rays and no clip clock at all. That matters: Crossroads_10
/// carried no actors before the boss and the frame is stall-bound.
#[derive(Clone, Copy)]
struct FalseKnightRuntime {
    controller: hk_sim::false_knight::FalseKnight,
    arena: hk_sim::boss::Arena,
    /// The `Head`'s own HealthManager, which `Health Check` restores to 40 so
    /// that emptying it spends a phase instead of killing anything.
    head: ActorHealth,
    animation_tick: u32,
    clip: hk_sim::false_knight::Clip,
    vx: i32,
    /// SetGravity2dScale, Q16: the serialized Rigidbody2D scale is zero and the
    /// FSM writes one of `GRAVITY_*` every time the body leaves the ground.
    gravity: i32,
    kinematic: bool,
    /// The `Hitter` child's DamageHero trigger, switched by the attack states.
    hitter: bool,
    /// The body's own DamageHero, which the stagger switches off.
    contact_damage: u16,
    /// `Spawns`, written by the SUMMON the controller just sent and drained by
    /// the one exit that can reach `FK Barrel Summon`. Zero the rest of the time.
    summon: u8,
    /// `Start Fall` drops the body out of the ceiling slab it is authored in;
    /// the shared spawn resolution runs once, there, rather than on load.
    separated: bool,
}
impl FalseKnightRuntime {
    /// The cooked clip for a source clip.
    ///
    /// Six of the thirty-four clips fit Crossroads_10's room-pack byte budget
    /// (the measurement is in docs/FALSE_KNIGHT.md), so the rest are shown as
    /// the nearest cooked pose rather than not shown at all. The substitutions
    /// are: every airborne clip borrows `Jump Antic`, every impact and every
    /// part of the roll borrows `Land`, the whole open-armour window borrows
    /// `Stun Opened`, the slam's wind-up and recovery borrow `Attack`, and what
    /// is left stands in `Idle`.
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::false_knight::Clip::*;
        let ActorController::FalseKnight { jump_antic_clip, land_clip, stun_opened_clip, attack_clip, .. } =
            spec.controller else { panic!("False Knight runtime with other metadata") };
        match self.clip {
            Turn => spec.turn_clip,
            JumpAntic | Jump | JumpAttackUp => jump_antic_clip,
            Land | JumpAttackHit1 | JumpAttackHit2 | JumpAttackHit3 | StunRoll | StunRollEnd
            | StunRecover | DeathFall | DeathLand => land_clip,
            StunOpen | StunOpened | StunHit => stun_opened_clip,
            // The wind-up and the recovery are the mace going up and coming
            // back; the struck pose is nearer to both than a standing one.
            Attack | AttackAntic | AttackRecover | Rage => attack_clip,
            // Idle, Run, Run Antic, Death Spaz and the one-pixel Blank the
            // Hitter overlay would have needed.
            _ => spec.walk_clip,
        }
    }
    /// `Turn R` writes the source scale x +1.3 and `Turn L` -1.3, and the cook
    /// takes the absolute scale, so a positive source scale is the art as
    /// authored, which this draw path expresses as -1.
    fn facing(&self) -> i32 {
        if self.controller.facing_right() { -1 } else { 1 }
    }
    /// The `Hitter` trigger in world coordinates, mirrored onto the side the
    /// body faces the way its parent transform's scale sign mirrors the child.
    fn hitter_box(&self, x: i32, y: i32) -> [i32; 4] {
        let b = hk_sim::false_knight::HITTER_BOX;
        if self.controller.facing_right() {
            [x + b[0], y + b[1], x + b[2], y + b[3]]
        } else {
            [x - b[2], y + b[1], x - b[0], y + b[3]]
        }
    }
    /// The exposed `Head` in world coordinates. Symmetric in x, so it does not
    /// mirror; it sits above the body box rather than inside it.
    fn head_box(&self, x: i32, y: i32) -> [i32; 4] {
        let b = hk_sim::false_knight::HEAD_BOX;
        [x + b[0], y + b[1], x + b[2], y + b[3]]
    }
}
// What a route can see of the fight. Without these the boss is the one feature
// in this port a replay cannot say anything about: it draws in a room the
// Knight can stand in without fighting, it never drops Geo, and its two health
// managers both restore instead of dying, so neither the wallet nor the kill
// counter moves until the whole thing is over.
//
// The counters are edges, incremented where the event happens. The live values
// below them are mirrored in exactly one place, `Actor::publish_false_knight`,
// because the boss is written back from three different exits and publishing at
// each of them is how one of them ends up stale.
/// The hero crossed `Battle Scene`'s trigger and the arena armed.
#[no_mangle] pub static mut HK_FK_TRIGGERED: u32 = 0;
/// `Start Fall` separated the body from the ceiling slab it hangs in.
#[no_mangle] pub static mut HK_FK_DROPPED: u32 = 0;
/// `Check Health` took the body to zero, so the armour rolled and opened.
#[no_mangle] pub static mut HK_FK_STAGGERS: u32 = 0;
/// `Health Check` took the exposed Head to zero during a stagger, which is the
/// only thing that advances a phase. Three of these reach the death sequence.
#[no_mangle] pub static mut HK_FK_CONVERSIONS: u32 = 0;
/// The HealthManager death event at the end of the 450-tick tail, which is a
/// different thing from the Head merely emptying: the last exposure empties it
/// too and only the tail that follows counts here.
#[no_mangle] pub static mut HK_FK_DEATHS: u32 = 0;
/// The body's own 65 hp and the exposed Head's 40, both of which restore.
#[no_mangle] pub static mut HK_FK_HP: i32 = 0;
#[no_mangle] pub static mut HK_FK_HEAD_HP: i32 = 0;
/// Out of `Dormant`, which is the whole difference between a fight that is
/// running and a boss hanging in the ceiling costing one AABB a tick.
#[no_mangle] pub static mut HK_FK_ACTIVE: u32 = 0;
/// The armour is open, so the nail reaches the Head rather than the body.
#[no_mangle] pub static mut HK_FK_EXPOSED: u32 = 0;
/// `Stunned Amount`, which selects the phase table.
#[no_mangle] pub static mut HK_FK_STUNNED: u32 = 0;
/// `Battle Control`'s own phase, as `hk_sim::boss::Phase` orders it, and its
/// `Activated` PersistentBoolItem.
#[no_mangle] pub static mut HK_FK_ARENA: u32 = 0;
#[no_mangle] pub static mut HK_FK_ACTIVATED: u32 = 0;
/// `summon`'s `Spawn`, once per `Falling Barrel` that left the pool, and once
/// per barrel that reached terrain or the hero. The two are separate because a
/// barrel the pool recycled under pressure never breaks, so a route that only
/// watched the spawns could not tell a fight the player dodged from one where
/// the eight of the rage evicted each other.
#[no_mangle] pub static mut HK_FK_BARRELS: u32 = 0;
#[no_mangle] pub static mut HK_FK_BARRELS_BROKEN: u32 = 0;

/// The Head's HealthManager: 40 hp with 0.15 s of invulnerability, no contact
/// damage of its own and no Geo, exactly as `host/false_knight.py` reads it.
const FALSE_KNIGHT_HEAD: hk_sim::EnemyParams = hk_sim::EnemyParams {
    health: hk_sim::false_knight::HEAD_HEALTH,
    contact_damage: 0,
    evasion_ticks: hk_sim::false_knight::HEAD_INVULNERABLE_TICKS,
    invincible: false,
    damage_override: false,
};
/// Visible enemy draws, which are tiles rather than actors: a frame past one
/// 64x64 animation slot contributes one draw and one animation key per tile.
/// The ceiling is `hk_cache::MAX_FRAME_TILES`, the slots the cache holds beyond
/// the four `main.rs` always reserves, because a draw the cache cannot key is
/// a draw that cannot reach VRAM. `main.rs` sizes its working set from
/// `hk_cache::MAX_REQUESTS` and holds the two constants to that relation.
pub const MAX_VISIBLE: usize = 20;
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnState {
    Pending,
    Ready,
    Blocked,
}
/// Surface-following Tiktik: the kinematic controller owns position, velocity
/// and rotation; this runtime casts its point rays against the resident terrain.
#[derive(Clone, Copy)]
struct ClimberRuntime {
    controller: hk_sim::climber::Climber,
    velocity: [i32; 2],
    animation_tick: u32,
    clip: hk_sim::climber::Clip,
    end_token: Option<u32>,
}
impl ClimberRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Climber { stun_clip, .. } = spec.controller else { panic!("Climber runtime with other metadata") };
        match self.clip { hk_sim::climber::Clip::Walk => spec.walk_clip, hk_sim::climber::Clip::Stun => stun_clip }
    }
    /// Quarter turns of the controller rotation (cardinal at rest, nearest during turns).
    fn quarter(&self) -> i32 {
        ((self.controller.rotation() + 45 * ONE).div_euclid(90 * ONE)).rem_euclid(4)
    }
}
/// Gravity-free Buzzer: the controller owns velocity and facing; this runtime
/// integrates the body against terrain with the bounded solver.
#[derive(Clone, Copy)]
struct VengeflyRuntime {
    controller: hk_sim::vengefly::Vengefly,
    animation_tick: u32,
    clip: hk_sim::vengefly::Clip,
}
impl VengeflyRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::vengefly::Clip;
        let ActorController::Vengefly { startle_clip, chase_clip, turn_fly_clip } = spec.controller
            else { panic!("Vengefly runtime with other metadata") };
        match self.clip {
            Clip::Idle => spec.walk_clip,
            Clip::TurnToIdle => spec.turn_clip,
            Clip::Startle => startle_clip,
            Clip::Chase => chase_clip,
            Clip::TurnToFly => turn_fly_clip,
        }
    }
    fn apply(&mut self, actions: hk_sim::vengefly::Actions) {
        use hk_sim::vengefly::Action;
        for action in actions.iter() {
            match action {
                Action::Play(clip, tick) => { self.clip = clip; self.animation_tick = tick; }
                // Velocity and facing are read from the controller; the
                // Startle one-shot is not presented.
                Action::Velocity(_) | Action::Facing(_) | Action::StartleSound => {}
            }
        }
    }
}
/// Bouncing Fly: the controller owns the flight angle and facing; this runtime
/// turns it into a 5.2 unit/s velocity and reports the blocked side.
#[derive(Clone, Copy)]
struct GruzzerRuntime {
    controller: hk_sim::gruzzer::Gruzzer,
    animation_tick: u32,
}
/// Rolling Baldur: the controller owns the roll speed, launches and clips;
/// this runtime runs the gravity body and reports wall/ground contacts.
#[derive(Clone, Copy)]
struct BaldurRuntime {
    controller: hk_sim::baldur::Baldur,
    animation_tick: u32,
    clip: hk_sim::baldur::Clip,
    vx: i32,
    wall: bool,
}
impl BaldurRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::baldur::Clip;
        let ActorController::Baldur { start_clip, roll_clip, stop_clip } = spec.controller
            else { panic!("Baldur runtime with other metadata") };
        match self.clip { Clip::Idle => spec.walk_clip, Clip::Start => start_clip, Clip::Roll => roll_clip, Clip::Stop => stop_clip }
    }
}
/// Aspid Hunter: the controller owns velocity, facing, clips and the shot cue.
#[derive(Clone, Copy)]
struct AspidRuntime {
    controller: hk_sim::aspid::Aspid,
    animation_tick: u32,
    clip: hk_sim::aspid::Clip,
}
impl AspidRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::aspid::Clip;
        let ActorController::Aspid { fire_clip, .. } = spec.controller else { panic!("Aspid runtime with other metadata") };
        match self.clip { Clip::Fly => spec.walk_clip, Clip::TurnToFly => spec.turn_clip, Clip::FireLong => fire_clip }
    }
}
/// Source `Spitter Shot R`: a gravity 0.05 dynamic body that damages the hero
/// on contact and plays Impact where it hits terrain or the hero.
///
/// The False Knight's `Falling Barrel` rides the same pool. It is the same
/// shape of object: a trigger box on a dynamic body that damages the hero and
/// ends on terrain, and its own `PersonalObjectPool` reserve is also eight, so
/// sharing costs neither a second array nor a second integrate-and-collide
/// loop. Only one of the two can ever be live, because Crossroads_10 is the
/// only room with a boss and it carries no Aspid.
pub const SHOTS: usize = 8;
const _: () = assert!(SHOTS >= hk_sim::false_knight::BARREL_POOL,
    "the shared projectile pool must hold FK Barrel Summon's whole reserve");
const SHOT_GRAVITY: i32 = 3 * ONE; // .05 * 60
const SHOT_HALF: [i32; 2] = [20992, 18432]; // .640625 x .5625 box at scale .7 * .8... kept at source size
const IMPACT_TICKS: u32 = 18; // six frames at 20 fps
#[derive(Clone, Copy)]
struct Shot {
    scene: u8,
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    animation_tick: u32,
    impact: bool,
    /// A `Falling Barrel` rather than an Aspid bullet: heavier, wider, and with
    /// no Impact clip, because `Break` hides the sprite and everything left of
    /// that state is particles and a one-shot.
    barrel: bool,
    shot_clip: u16,
    impact_clip: u16,
}
impl Shot {
    fn gravity(&self) -> i32 {
        if self.barrel { hk_sim::false_knight::BARREL_GRAVITY } else { SHOT_GRAVITY }
    }
    fn half(&self) -> [i32; 2] {
        if self.barrel { hk_sim::false_knight::BARREL_HALF } else { SHOT_HALF }
    }
    fn damage(&self) -> u16 {
        if self.barrel { hk_sim::false_knight::BARREL_DAMAGE } else { 1 }
    }
}
/// Does the movement segment cross any terrain edge (closed endpoints)?
fn segment_hits_terrain(a: [i32; 2], b: [i32; 2], count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> bool {
    let (ox, oy) = (a[0] as i64, a[1] as i64);
    let (rx, ry) = (b[0] as i64 - ox, b[1] as i64 - oy);
    for i in 0..count {
        let e = edge(i);
        if e == [0; 4] { continue; }
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
/// `GetDistance` between two transforms, Q16. World coordinates stay inside
/// +/-512 units, so the squares stay well below the i64 range.
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
/// Rotate a local offset by quarter turns counter-clockwise.
fn rotate_quarter(v: [i32; 2], quarter: i32) -> [i32; 2] {
    match quarter.rem_euclid(4) { 0 => v, 1 => [-v[1], v[0]], 2 => [-v[0], -v[1]], _ => [v[1], -v[0]] }
}
/// Nearest terrain hit of a Climber point ray, as the source raycast on layer
/// 256: origin = position + R(rotation) * (scale * local_origin), direction
/// rotated without scale, bounded by the ray length.
fn climber_ray_hit(ray: hk_sim::climber::Ray, count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> Option<[i32; 2]> {
    let quarter = ((ray.rotation_degrees_q16 + 45 * ONE).div_euclid(90 * ONE)).rem_euclid(4);
    let local = [ray.local_origin[0] * ray.scale_x_sign as i32, ray.local_origin[1]];
    let o = rotate_quarter(local, quarter);
    let origin = [ray.position[0] + o[0], ray.position[1] + o[1]];
    let d = rotate_quarter(ray.local_direction, quarter);
    let end = [origin[0] as i64 + (d[0] as i64 * ray.length as i64 >> 16), origin[1] as i64 + (d[1] as i64 * ray.length as i64 >> 16)];
    let (ox, oy) = (origin[0] as i64, origin[1] as i64);
    let (rx, ry) = (end[0] - ox, end[1] - oy);
    let mut best: Option<(i64, i64, [i32; 2])> = None;
    for i in 0..count {
        let e = edge(i);
        if e == [0; 4] { continue; }
        let (ax, ay) = (e[0] as i64, e[1] as i64);
        let (sx, sy) = (e[2] as i64 - ax, e[3] as i64 - ay);
        let denominator = rx * sy - ry * sx;
        if denominator == 0 { continue; }
        // origin + t*r = a + u*s, both parameters in [0, 1], scaled by the denominator.
        let (qx, qy) = (ax - ox, ay - oy);
        let t_num = qx * sy - qy * sx;
        let u_num = qx * ry - qy * rx;
        let (t_num, u_num, den) = if denominator < 0 { (-t_num, -u_num, -denominator) } else { (t_num, u_num, denominator) };
        if t_num < 0 || t_num > den || u_num < 0 || u_num > den { continue; }
        // Compare the ray parameter as a fraction t_num/den across edges.
        let nearer = best.is_none_or(|(t, d, _): (i64, i64, [i32; 2])| t_num * d < t * den);
        if nearer {
            let point = [(ox + rx * t_num / den) as i32, (oy + ry * t_num / den) as i32];
            best = Some((t_num, den, point));
        }
    }
    best.map(|(_, _, p)| p)
}
const EDGES:usize=128;
/// Simulation neighbourhood around the active view, in Q16 units (one view).
const NEAR:[i32;2]=[24*ONE,16*ONE];
// Round-robin eviction thrashes once the advancing actors cycle through more
// distinct views than slots; the neighbourhood gate keeps that count small.
const EDGE_SLOTS:usize=4;
/// Per-view terrain copies for actor physics. Decoding 128 edges of a
/// non-active view per actor per tick was the largest enemy cost.
#[cfg(not(test))]
struct ActorEdges {
    edges:[[[i32;4];EDGES];EDGE_SLOTS],
    keys:[(usize,usize,u32);EDGE_SLOTS],
    next:usize,
}
#[cfg(not(test))]
static mut ACTOR_EDGES:ActorEdges=ActorEdges {edges:[[[0;4];EDGES];EDGE_SLOTS],keys:[(usize::MAX,0,0);EDGE_SLOTS],next:0};
#[cfg(not(test))]
fn actor_edges<'a>(state:&State,active:&Region,active_room:&Room,target:&Region,target_room:&Room)->&'a [[i32;4]] {
    let count=target_room.counts[5];
    // The scene payload behind a global id is immutable within an edge epoch
    // (world re-admission bumps it), so the copy is keyed by ids, not by the
    // transient Room view address. Geo/Lifeblood exclusions are indices into
    // the active room, so the active view is part of the key.
    let key=(target.global_id,active.global_id,state.edge_epoch);
    // Single-threaded main loop; the returned slice is read before the next fill.
    let cache=unsafe {&mut *core::ptr::addr_of_mut!(ACTOR_EDGES)};
    let slot=match cache.keys.iter().position(|k|*k==key) {
        Some(slot)=>slot,
        None=>{
            let slot=cache.next;cache.next=(slot+1)%EDGE_SLOTS;
            state.fill_edges_in_view(active,active_room,target,target_room,&mut cache.edges[slot][..count]);
            cache.keys[slot]=key;
            slot
        }
    };
    &cache.edges[slot][..count]
}
#[derive(Clone, Copy)]
struct Actor {
    scene: usize,
    source_id: u32,
    x: i32,
    y: i32,
    vy: i32,
    grounded: bool,
    spawn_state: SpawnState,
    health: ActorHealth,
    walk: WalkState,
    recoil_left: u16,
    recoil: [i32; 2],
    corpse: Option<hk_sim::Corpse>,
    geo_paid: bool,
    /// A Dream Nail slash pays SOUL once per actor, as the source's
    /// `EnemyDreamnailReaction` clears its state after the first impact.
    dream_taken: bool,
    runtime: Runtime,
}
/// One controller runtime per actor; the Crawler keeps only the shared WalkState.
#[derive(Clone, Copy)]
enum Runtime {
    Walker,
    Runner(RunnerRuntime),
    Climber(ClimberRuntime),
    Vengefly(VengeflyRuntime),
    Gruzzer(GruzzerRuntime),
    Baldur(BaldurRuntime),
    Aspid(AspidRuntime),
    /// No senses and no body: only the looping clip's clock advances.
    Static { animation_tick: u32 },
    FalseKnight(FalseKnightRuntime),
}
impl Actor {
    fn runner(&self) -> Option<RunnerRuntime> { if let Runtime::Runner(r) = self.runtime { Some(r) } else { None } }
    fn runner_mut(&mut self) -> Option<&mut RunnerRuntime> { if let Runtime::Runner(r) = &mut self.runtime { Some(r) } else { None } }
    fn climber(&self) -> Option<ClimberRuntime> { if let Runtime::Climber(c) = self.runtime { Some(c) } else { None } }
    fn climber_mut(&mut self) -> Option<&mut ClimberRuntime> { if let Runtime::Climber(c) = &mut self.runtime { Some(c) } else { None } }
    fn vengefly(&self) -> Option<VengeflyRuntime> { if let Runtime::Vengefly(v) = self.runtime { Some(v) } else { None } }
    fn gruzzer(&self) -> Option<GruzzerRuntime> { if let Runtime::Gruzzer(g) = self.runtime { Some(g) } else { None } }
    fn baldur(&self) -> Option<BaldurRuntime> { if let Runtime::Baldur(b) = self.runtime { Some(b) } else { None } }
    fn baldur_mut(&mut self) -> Option<&mut BaldurRuntime> { if let Runtime::Baldur(b) = &mut self.runtime { Some(b) } else { None } }
    fn aspid(&self) -> Option<AspidRuntime> { if let Runtime::Aspid(a) = self.runtime { Some(a) } else { None } }
    fn immobile(&self) -> Option<u32> { if let Runtime::Static { animation_tick } = self.runtime { Some(animation_tick) } else { None } }
    fn false_knight(&self) -> Option<FalseKnightRuntime> { if let Runtime::FalseKnight(b) = self.runtime { Some(b) } else { None } }
    /// Write the boss back and mirror what a route reads off it.
    ///
    /// The runtime is `Copy` and is taken out, stepped and put back from three
    /// places: the tick that advances it, the nail pass that strikes it, and
    /// the dead path that keeps the arena counting through `End Wait`. Every
    /// one of those goes through here, so the live values cannot be published
    /// by two of the three and quietly go stale in the other.
    ///
    /// Dormant is free and stays free. None of these can move while the boss
    /// hangs in the ceiling waiting on one AABB, and `sync_region` published
    /// them once when the actor was seated, so the tick the room spends most of
    /// its life in pays nothing for them.
    fn seat_false_knight(&mut self, boss: FalseKnightRuntime) {
        self.runtime = Runtime::FalseKnight(boss);
        if boss.controller.phase() != hk_sim::false_knight::Phase::Dormant {
            self.publish_false_knight(boss);
        }
    }
    /// The one place the fight's live values reach a route.
    fn publish_false_knight(&self, boss: FalseKnightRuntime) {
        unsafe {
            HK_FK_HP = self.health.hp as i32;
            HK_FK_HEAD_HP = boss.head.hp as i32;
            HK_FK_ACTIVE = u32::from(boss.controller.phase() != hk_sim::false_knight::Phase::Dormant);
            HK_FK_EXPOSED = u32::from(boss.controller.head_exposed());
            HK_FK_STUNNED = boss.controller.stunned() as u32;
            HK_FK_ARENA = boss.arena.phase() as u32;
            HK_FK_ACTIVATED = u32::from(boss.arena.activated());
        }
    }
    fn is_flyer(&self) -> bool { matches!(self.runtime, Runtime::Vengefly(_) | Runtime::Gruzzer(_) | Runtime::Baldur(_) | Runtime::Aspid(_)) }
    fn new(scene: usize, spec: &ActorSpec) -> Self {
        // Stable per-source seed, not a claim to reproduce Unity's global RNG.
        let seed = spec
            .source_id
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        Self {
            scene,
            source_id: spec.source_id,
            x: spec.x,
            y: spec.y,
            vy: 0,
            grounded: false,
            spawn_state: SpawnState::Pending,
            health: ActorHealth::new(spec.health),
            walk: WalkState::new(
                spec.initial_direction,
                spec.random_start_direction && seed & 0x80000000 != 0,
            ),
            recoil_left: 0,
            recoil: [0; 2],
            corpse: None,
            geo_paid: false,
            dream_taken: false,
            runtime: match spec.controller {
                ActorController::Crawler => Runtime::Walker,
                ActorController::Runner { params, .. } => {
                    // A mirrored placement starts facing right; the cooker takes
                    // the sign from the transform, so both are supported now.
                    assert!(spec.initial_direction.abs() == 1 && !spec.random_start_direction,
                        "unsupported Runner initial facing variant");
                    Runtime::Runner(RunnerRuntime {
                        controller: hk_sim::runner::Runner::with_facing(params, spec.initial_direction as i8), rng: seed,
                        vx: 0, animation_tick: 0 })
                }
                ActorController::Climber { start_right, rotation_q16, .. } => Runtime::Climber(ClimberRuntime {
                    // Unit positive scale is the recognizer's contract, so the
                    // authored rotation is the whole of the starting pose.
                    controller: hk_sim::climber::Climber::new([spec.x, spec.y], rotation_q16, 1, start_right).expect("validated Climber placement"),
                    velocity: [0; 2], animation_tick: 0, clip: hk_sim::climber::Clip::Walk, end_token: None }),
                ActorController::Vengefly { .. } => Runtime::Vengefly(VengeflyRuntime {
                    controller: hk_sim::vengefly::Vengefly::new([spec.x, spec.y], seed), animation_tick: 0, clip: hk_sim::vengefly::Clip::Idle }),
                ActorController::Gruzzer => Runtime::Gruzzer(GruzzerRuntime { controller: hk_sim::gruzzer::Gruzzer::new(seed), animation_tick: 0 }),
                ActorController::Baldur { .. } => Runtime::Baldur(BaldurRuntime { controller: hk_sim::baldur::Baldur::new(seed), animation_tick: 0,
                    clip: hk_sim::baldur::Clip::Idle, vx: 0, wall: false }),
                ActorController::Aspid { start_alert, .. } => {
                    let controller = if start_alert { hk_sim::aspid::Aspid::new_alert([spec.x, spec.y], seed) }
                        else { hk_sim::aspid::Aspid::new([spec.x, spec.y], seed) };
                    Runtime::Aspid(AspidRuntime { controller, animation_tick: 10, clip: hk_sim::aspid::Clip::Fly })
                }
                ActorController::Static { .. } => Runtime::Static { animation_tick: 0 },
                // `initial_direction` carries the sign the cooker read off the
                // placement's transform scale, and a positive source scale is
                // the art as authored, which that field writes as -1.
                //
                // `Pause` -> `Init`, which the source takes on the arena's first
                // frame. The `Battle Scene`'s PersistentBoolItem and
                // `falseKnightFirstPlop` live in the save record's PlayerData
                // reserve, so a cleared arena is still cleared after a scene
                // reload, a death respawn or a boot from the card.
                ActorController::FalseKnight { .. } => {
                    let arena = hk_sim::boss::Arena::new(
                        crate::script::player_data(crate::script::FIELD_FALSE_KNIGHT_ARENA) != 0);
                    let first_plop =
                        crate::script::player_data(crate::script::FIELD_FALSE_KNIGHT_FIRST_PLOP) != 0;
                    // Of `Init`'s actions the gates are the only ones with
                    // anywhere to go: `CameraLockArea B` is authored world the
                    // cook already lays down, and the arena floor the defeated
                    // branch activates is cooked terrain nothing can break.
                    crate::battle_gates::arena_entry(scene,
                        arena.enter().contains(hk_sim::boss::Action::QuickOpenGates));
                    Runtime::FalseKnight(FalseKnightRuntime {
                        controller: hk_sim::false_knight::FalseKnight::new(seed, first_plop),
                        arena,
                        head: ActorHealth::new(FALSE_KNIGHT_HEAD),
                        animation_tick: 0, clip: hk_sim::false_knight::Clip::Idle,
                        vx: 0, gravity: hk_sim::false_knight::GRAVITY_IDLE,
                        kinematic: true, hitter: false, contact_damage: spec.health.contact_damage,
                        summon: 0, separated: false })
                }
            },
        }
    }
    fn local_bounds(&self, spec: &ActorSpec) -> [i32; 4] {
        let b = spec.bounds;
        if let Some(climber) = self.climber() {
            // The source box collider turns with the transform; at rest the
            // rotation is cardinal, so the rotated box is again axis aligned.
            // The cooked box is already in the spawn pose, so only the turns
            // taken since spawn rotate it: a ceiling placement starts at 180
            // and its authored box is the upside-down one.
            let ActorController::Climber { rotation_q16, .. } = spec.controller
                else { panic!("Climber runtime with other metadata") };
            let q = climber.quarter() - ((rotation_q16 + 45 * ONE).div_euclid(90 * ONE)).rem_euclid(4);
            let corners = [rotate_quarter([b[0], b[1]], q), rotate_quarter([b[2], b[3]], q)];
            return [corners[0][0].min(corners[1][0]), corners[0][1].min(corners[1][1]),
                corners[0][0].max(corners[1][0]), corners[0][1].max(corners[1][1])];
        }
        if self.is_flyer() || self.walk.direction == spec.initial_direction {
            b
        } else {
            [-b[2], b[1], -b[0], b[3]]
        }
    }
    /// One 60 Hz step of the kinematic Climber: motion along its surface
    /// direction, then the controller's ray-driven sensing or turn/stun timers.
    fn advance_climber(&mut self, c: &mut ClimberRuntime, count: usize, edge: &impl Fn(usize) -> [i32; 4]) {
        use hk_sim::climber::{Action, Phase};
        c.animation_tick = c.animation_tick.saturating_add(1);
        let cast = |ray| climber_ray_hit(ray, count, edge);
        let apply = |actor: &mut Self, c: &mut ClimberRuntime, actions: hk_sim::climber::Actions| {
            for action in actions.iter() {
                match action {
                    Action::Position(p) => { actor.x = p[0]; actor.y = p[1]; }
                    Action::Velocity(v) => c.velocity = v,
                    Action::Rotation(_) => {}
                    Action::Play(clip) => { c.clip = clip; c.animation_tick = 0; }
                    Action::AwaitEndOfFrame(token) => c.end_token = Some(token),
                }
            }
        };
        match c.controller.phase() {
            Phase::AwaitAttachment => {
                let ray = c.controller.attachment_ray().expect("attachment ray while awaiting");
                let actions = c.controller.attach(cast(ray)).expect("Climber attachment inside validated bounds");
                apply(self, c, actions);
                // StartCoroutine(Walk) runs its first iteration immediately.
                let actions = c.controller.walk_frame([self.x, self.y], |r| cast(r).is_some()).expect("Climber walk inside validated bounds");
                apply(self, c, actions);
            }
            Phase::Walking => {
                self.x += c.velocity[0] / 60;
                self.y += c.velocity[1] / 60;
                let actions = c.controller.walk_frame([self.x, self.y], |r| cast(r).is_some()).expect("Climber walk inside validated bounds");
                apply(self, c, actions);
            }
            Phase::Turning => {
                let actions = c.controller.turn_frame([self.x, self.y]).expect("Climber turn inside validated bounds");
                apply(self, c, actions);
            }
            Phase::Stunned => { let actions = c.controller.stun_tick(); apply(self, c, actions); }
            Phase::StunEndOfFrame => {
                if let Some(token) = c.end_token.take() {
                    let actions = c.controller.end_of_frame(token);
                    apply(self, c, actions);
                }
            }
            Phase::Dead => {}
        }
        self.walk.direction = 1;
    }
    fn bounds(&self, spec: &ActorSpec) -> [i32; 4] {
        let b = self.local_bounds(spec);
        [self.x + b[0], self.y + b[1], self.x + b[2], self.y + b[3]]
    }
    /// Source `Check Health` on the body and `Health Check` on the `Head`.
    /// Neither HealthManager lets its owner die from a nail: zero on the body
    /// restores 65 and staggers, zero on the exposed Head restores 40 and ends
    /// the stagger, and only the third of those reaches the death sequence.
    ///
    /// `hits` is the attacker's own shape test against a world box, because the
    /// nail is a polygon and the Vengeful Spirit ball is a box. Returns whether
    /// it reached a box at all, so the caller answers it with the same recoil an
    /// invulnerable enemy gets, and what the hit was.
    #[inline(never)]
    fn strike_false_knight(&mut self, boss: &mut FalseKnightRuntime, spec: &ActorSpec,
                           hits: impl Fn([i32; 4]) -> bool, damage: u16) -> (bool, Hit) {
        if boss.controller.head_exposed() {
            // `Opened` drops the Head hitbox out of its parked position; the
            // armour around it is invincible for as long as the window lasts.
            if !hits(boss.head_box(self.x, self.y)) {
                return (false, Hit::Ignored);
            }
            let hit = boss.head.hit(FALSE_KNIGHT_HEAD, damage);
            let actions = if boss.head.dead {
                // `Health Check`: SetHP 40 then STUN END, so the Head survives
                // to be exposed again. Three of these are the whole fight.
                boss.head = ActorHealth::new(FALSE_KNIGHT_HEAD);
                boss.controller.head_reached_zero()
            } else if hit == Hit::Damaged {
                boss.controller.head_hit()
            } else {
                return (true, hit);
            };
            self.apply_false_knight(boss, actions);
            return (true, Hit::Damaged);
        }
        if !hits(self.bounds(spec)) {
            return (false, Hit::Ignored);
        }
        if boss.controller.invincible() {
            return (true, Hit::Blocked);
        }
        let hit = self.health.hit(spec.health, damage);
        if !self.health.dead {
            return (true, hit);
        }
        // `Check Health`: ZERO HP sends STUN and SetHP 65, so the body never
        // dies. Reaching zero is what staggers it, which is the only way in.
        self.health = ActorHealth::new(spec.health);
        let actions = boss.controller.body_reached_zero();
        // `Check Health` is refused while the boss is invincible or still
        // dropping, so the counter follows what the controller did rather than
        // what the nail asked for: an empty list is a stagger that did not
        // happen and the body is back at 65 either way.
        if !actions.is_empty() { unsafe { HK_FK_STAGGERS = HK_FK_STAGGERS.saturating_add(1) } }
        self.apply_false_knight(boss, actions);
        (true, Hit::Damaged)
    }
    // Actors and corpses share the materialized terrain of their owning view;
    // the copy is rebuilt only when that view or the exclusion epoch changes.
    #[inline(never)]
    fn advance_in_view(&mut self,spec:&ActorSpec,state:&State,active:&Region,
        active_room:&Room,target:&Region,target_room:&Room, context:RunnerContext, emit:&mut impl FnMut(RunnerEvent)) {
        let count=target_room.counts[5];
        if count<=EDGES {
            // Tests swap rooms behind one region fixture and run in parallel:
            // they fill a private copy instead of the shared keyed slots.
            #[cfg(test)]
            {
                let mut edges=[[0;4];EDGES];
                state.fill_edges_in_view(active,active_room,target,target_room,&mut edges[..count]);
                return self.advance_body(spec,active_room,count,|i|edges[i],context,emit);
            }
            #[cfg(not(test))]
            {
                let edges=actor_edges(state,active,active_room,target,target_room);
                self.advance_body(spec,active_room,count,|i|edges[i],context,emit);
            }
        } else {
            self.advance_body(spec,active_room,count,|i|state.edge_in_view(active,active_room,target,target_room,i),context,emit);
        }
    }
    fn advance_body(&mut self,spec:&ActorSpec,room:&Room,count:usize,edge:impl Fn(usize)->[i32;4],context:RunnerContext,emit:&mut impl FnMut(RunnerEvent)) {
        if self.health.dead {
            if let (Some(corpse),Some(corpse_spec))=(&mut self.corpse,spec.corpse) {
                corpse.tick(corpse_spec,count,edge);
            }
            // `End Wait` runs after the boss's death event, so the arena has to
            // keep counting for the two seconds the source waits before BG OPEN.
            if let Some(mut boss)=self.false_knight() {
                let actions=boss.arena.tick();
                self.apply_arena(&mut boss,actions);
                self.seat_false_knight(boss);
            }
        } else {self.advance(spec,room,count,edge,context,emit);}
    }
    fn advance(&mut self, spec: &ActorSpec, room:&Room, count: usize, edge: impl Fn(usize) -> [i32; 4], context:RunnerContext,emit:&mut impl FnMut(RunnerEvent)) {
        if self.spawn_state == SpawnState::Blocked {
            return;
        }
        if let Some(mut climber) = self.climber() {
            // Kinematic: no gravity solve, no spawn resolution; the source
            // attachment ray places the body on its surface.
            self.spawn_state = SpawnState::Ready;
            self.advance_climber(&mut climber, count, &edge);
            self.runtime = Runtime::Climber(climber);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut fly) = self.vengefly() {
            self.spawn_state = SpawnState::Ready;
            self.advance_vengefly(&mut fly, spec, count, &edge, context);
            self.runtime = Runtime::Vengefly(fly);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut fly) = self.gruzzer() {
            self.spawn_state = SpawnState::Ready;
            self.advance_gruzzer(&mut fly, spec, count, &edge, context);
            self.runtime = Runtime::Gruzzer(fly);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut roller) = self.baldur() {
            self.advance_baldur(&mut roller, spec, count, &edge, context);
            self.runtime = Runtime::Baldur(roller);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut aspid) = self.aspid() {
            self.spawn_state = SpawnState::Ready;
            self.advance_aspid(&mut aspid, spec, count, &edge, context, emit);
            self.runtime = Runtime::Aspid(aspid);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut boss) = self.false_knight() {
            self.advance_false_knight(&mut boss, spec, count, &edge, context, emit);
            self.seat_false_knight(boss);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(animation_tick) = self.immobile() {
            // No Rigidbody2D, so the source object cannot be pushed out of
            // terrain or fall: it keeps its authored transform and loops.
            self.spawn_state = SpawnState::Ready;
            self.runtime = Runtime::Static { animation_tick: animation_tick.saturating_add(1) };
            return;
        }
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        // Installed Physics2DSettings: gravity.y=-60, max translation speed=100;
        // supported Crawler Rigidbody2D: gravityScale=1, linearDamping=0.
        // runner-crossroads70/enemy-trace.csv: first Runner settles y=2.45249987.
        // crossroads68/landing/room.hk edge118 is floor y=1 at x37..49;
        // body bottom=-1.4375 gives .015 separation (Q16 983). Preserve that
        // grounded clearance in the bounded solver; contact/Sweep/draw bounds
        // remain the source box. Otherwise its lowest horizontal Sweep lies
        // exactly on Terrain and the segment model invents a perpetual wall.
        let foot_skin=if self.runner().is_some() {983}else{0};
        let gravity = if let ActorController::Runner { params, .. } = spec.controller { params.gravity } else { 60 * ONE };
        let mut p = Params { gravity, fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1]-foot_skin, top: b[3], ..Params::ZERO };
        if self.spawn_state == SpawnState::Pending {
            // The source Rigidbody2D may begin slightly inside Terrain. Resolve
            // once when its collision apron is resident, never on grid swaps or
            // later jump/recoil motion. Unsupported overlapping geometry leaves
            // this actor suspended until a scene reset, without retrying per tick.
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, &edge) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            self.x = body.x - offset;
            self.y = body.y;
            self.spawn_state = SpawnState::Ready;
        }
        let vx = if let Some(mut runner) = self.runner() {
            runner.animation_tick = runner.animation_tick.saturating_add(1);
            let senses = self.runner_senses(&runner, spec, room, context, count, &edge);
            let actions = runner.controller.step(senses, |w, e| RunnerRuntime::choose(&mut runner.rng, w, e));
            self.apply_runner_actions(&mut runner, actions, emit);
            let vx = runner.vx;
            self.runtime = Runtime::Runner(runner);
            vx
        } else {
            let senses = hk_sim::walker_senses(self.bounds(spec), self.walk.direction, count, &edge);
            self.walk.tick(spec.walk, senses.0, senses.1, senses.2)
        };
        // A completed source turn can mirror the asymmetric collider offset.
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        p.half_width = (b[2] - b[0]) / 2;
        p.bottom = b[1]-foot_skin;
        p.top = b[3];
        p.speed = vx.abs();
        body.step(p, vx.signum(), false, count, &edge);
        self.vy = body.vy;
        self.grounded = body.grounded;
        if self.recoil_left != 0 {
            // Recoil.UpdatePhysics sweeps a separate transform displacement;
            // it does not replace the walking rigidbody velocity. Reuse the
            // bounded terrain solver for that displacement, preserving gravity.
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            p.gravity = 0;
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, &edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        assert!(
            self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE,
            "enemy left validated Q16 scene bounds"
        );
    }
    fn vengefly_senses(&self, context: RunnerContext, count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> hk_sim::vengefly::Senses {
        use hk_sim::runner_senses as q;
        let pos = [self.x, self.y];
        // Alert Range New: a 7.804-unit circle trigger against the hero body box.
        let hb = context.hero_body;
        let dx = (pos[0].clamp(hb[0], hb[2]) - pos[0]) as i64;
        let dy = (pos[1].clamp(hb[1], hb[3]) - pos[1]) as i64;
        const RADIUS: i64 = 511463; // 7.804 Q16
        let in_alert_range = dx * dx + dy * dy <= RADIUS * RADIUS;
        let can_see_hero = match q::line_of_sight(pos, context.hero, in_alert_range, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Vengefly LOS outside validated coordinates"),
        };
        hk_sim::vengefly::Senses { position: pos, hero: context.hero, can_see_hero }
    }
    /// One 60 Hz step of the Buzzer: FSM tick, then the gravity-free body
    /// moves by its velocity against terrain, plus any Recoil displacement.
    fn advance_vengefly(&mut self, fly: &mut VengeflyRuntime, spec: &ActorSpec, count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        fly.animation_tick = fly.animation_tick.saturating_add(1);
        let senses = self.vengefly_senses(context, count, edge);
        let actions = fly.controller.tick(senses);
        fly.apply(actions);
        self.walk.direction = fly.controller.facing();
        let b = spec.bounds;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let v = fly.controller.velocity();
        let mut body = Player::spawn(self.x + offset, self.y);
        let mut p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        body.vy = v[1];
        body.step(p, v[0].signum(), false, count, edge);
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
    }
    /// One 60 Hz step of the Gruzzer: camera wake, constant-speed flight along
    /// the controller angle against terrain, Recoil displacement, then the
    /// blocked side (up, right, down, left, as CheckCollisionSide) re-aims.
    fn advance_gruzzer(&mut self, fly: &mut GruzzerRuntime, spec: &ActorSpec, count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        use hk_sim::gruzzer::{Phase, Side, SPEED};
        fly.animation_tick = fly.animation_tick.saturating_add(1);
        fly.controller.tick(context.camera, [self.x, self.y, 0]);
        self.walk.direction = fly.controller.facing();
        let v = if fly.controller.phase() == Phase::Flying {
            crate::world::debris::rotate([SPEED, 0], fly.controller.angle())
        } else { [0; 2] };
        let b = spec.bounds;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let mut body = Player::spawn(self.x + offset, self.y);
        let mut p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        body.vy = v[1];
        body.step(p, v[0].signum(), false, count, edge);
        let moved = [body.x - (self.x + offset), body.y - self.y];
        let blocked_x = moved[0] != v[0] / 60;
        let blocked_y = moved[1] != v[1] / 60;
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        let side = if blocked_y && v[1] > 0 { Some(Side::Up) }
            else if blocked_x && v[0] > 0 { Some(Side::Right) }
            else if blocked_y && v[1] < 0 { Some(Side::Down) }
            else if blocked_x && v[0] < 0 { Some(Side::Left) }
            else { None };
        if let Some(side) = side { fly.controller.bonk(side); }
    }
    /// `Battle Control`'s own actions. The camera lock is the one this port
    /// already answers, because `CameraLockArea B` is an authored lock area the
    /// world cooks like any other rather than something the arena switches.
    fn apply_arena(&mut self, boss: &mut FalseKnightRuntime, actions: hk_sim::boss::Actions) {
        for action in actions.iter() {
            match action {
                hk_sim::boss::Action::StartBattle => {
                    let started = boss.controller.battle_start();
                    self.apply_false_knight(boss, started);
                }
                // The broadcast reaches every gate in the room, not only the two
                // at the arena ends, which is why the gate world is keyed by
                // scene. All five of Crossroads_10's are cooked terrain, so this
                // seals the arena rather than only shutting the middle of it:
                // see game/src/battle_gates.rs.
                hk_sim::boss::Action::CloseGates => crate::battle_gates::close(self.scene),
                hk_sim::boss::Action::OpenGates
                | hk_sim::boss::Action::QuickOpenGates => crate::battle_gates::open(self.scene),
                // `End Wait` writes `Activated` before its two-second wait, so a
                // save taken during the wait already counts as cleared. This
                // write is the same order: the record reaches the card at the
                // next bench, which is the only thing that ever writes it.
                hk_sim::boss::Action::Persist => {
                    crate::script::set_player_data(crate::script::FIELD_FALSE_KNIGHT_ARENA, 1)
                }
                // Still not presented, and each for its own reason.
                // `KillMinions` has no target because the three pre-battle
                // Zombies are recorded rather than admitted; `ActivateFloor`
                // names the arena floor, which is cooked terrain nothing can
                // break; and `CameraLock` is the authored `CameraLockArea B`.
                hk_sim::boss::Action::ActivateFloor
                | hk_sim::boss::Action::CameraLock(_)
                | hk_sim::boss::Action::KillMinions => {}
            }
        }
    }
    /// One `FalseyControl` action list against the guest body.
    fn apply_false_knight(&mut self, boss: &mut FalseKnightRuntime, actions: hk_sim::false_knight::Actions) {
        use crate::camera::Shake;
        use hk_sim::false_knight::{Action, Effect};
        for action in actions.iter() {
            match action {
                // The source splits the attack art between the body, which
                // plays `Blank`, and the `Hitter` overlay. This port draws one
                // actor, so the overlay's clip plays on the body and the Blank
                // that would have hidden it is dropped.
                Action::Play(clip) | Action::PlayHitter(clip) => {
                    if clip != hk_sim::false_knight::Clip::Blank {
                        boss.clip = clip;
                        boss.animation_tick = 0;
                    }
                }
                Action::Velocity([x, y]) => { boss.vx = x; self.vy = y; self.grounded = false; }
                Action::VelocityX(x) => boss.vx = x,
                Action::Gravity(g) => boss.gravity = g,
                Action::Facing(f) => self.walk.direction = -f,
                Action::Hitter(on) => boss.hitter = on,
                Action::ContactDamage(d) => boss.contact_damage = d,
                Action::Kinematic(k) => { boss.kinematic = k; if k { self.vy = 0; boss.vx = 0; } }
                Action::KillAllEnemies => {
                    let cleared = boss.arena.kill_all_enemies();
                    self.apply_arena(boss, cleared);
                }
                Action::Died => {
                    boss.arena.enemy_died();
                    self.health.dead = true;
                    unsafe { HK_FK_DEATHS = HK_FK_DEATHS.saturating_add(1) }
                }
                // `Pause Long` sets `falseKnightFirstPlop`, so every later
                // stagger takes the short pause, in this fight and in every
                // one after it that the card carries.
                Action::SetFirstPlop => {
                    crate::script::set_player_data(crate::script::FIELD_FALSE_KNIGHT_FIRST_PLOP, 1)
                }
                Action::Staggered(_) => unsafe {
                    HK_FK_CONVERSIONS = HK_FK_CONVERSIONS.saturating_add(1)
                },
                // The `SendEventByName` and the `AudioPlaySimple` that the same
                // source states carry. The actor loop in `tick` skips every
                // actor whose `scene` is not the active region's, so the only
                // fight that can reach the one camera is the one the hero is in
                // the scene of, which is the source's own reach: its
                // `CameraShake` is a single object under `_GameCameras`.
                Action::Effect(effect) => match effect {
                    Effect::LandingShake => {
                        crate::camera::request(Shake::Average);
                        crate::audio::boss_land();
                    }
                    Effect::Landing => crate::audio::boss_land(),
                    Effect::Swing => crate::audio::boss_swing(),
                    Effect::BigShake => crate::camera::request(Shake::Big),
                    Effect::RageSlam => crate::camera::request(Shake::Average),
                    Effect::JumpShake => crate::camera::request(Shake::Kill),
                },
                // SUMMON, with `Spawns` written. The summoner is a different
                // object with its own clock, so this is only the send: it is
                // held here and drained once, in `advance_false_knight`, rather
                // than emitted from each of the three places the boss is
                // written back from. Only `tick` ever produces one, and holding
                // it means the other two exits cannot quietly drop it.
                Action::SummonBarrels(spawns) => boss.summon = spawns,
                // `Invincible` and `HeadExposed` are read back off the phase
                // rather than latched, so there is one source of truth for
                // which target the nail reaches.
                //
                // The rest is recorded, not presented: there is no shockwave
                // object, and the arena floor is cooked terrain that nothing
                // can crack or destroy. docs/FALSE_KNIGHT.md carries what each
                // of those costs against the budget that refuses it.
                Action::Invincible(_) | Action::HeadExposed(_) | Action::Shockwave { .. }
                | Action::CrackFloor | Action::BreakFloor => {}
            }
        }
    }
    /// One 60 Hz step of the False Knight and the arena it drives.
    ///
    /// Two very different costs, and `Phase::Dormant` is what keeps them apart.
    /// Until `BATTLE START` the source body hangs kinematic in the ceiling slab
    /// above the arena and the only thing running is `Battle Control`'s
    /// `Detect`, which is one box against the hero and nothing else: no solver,
    /// no terrain ray, no clip clock. That gate is load-bearing, because
    /// Crossroads_10 carried no actors at all before the boss and the frame is
    /// stall-bound.
    #[inline(never)]
    fn advance_false_knight(&mut self, boss: &mut FalseKnightRuntime, spec: &ActorSpec,
                            count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext,
                            emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::false_knight as fk;
        let ActorController::FalseKnight { trigger, barrel_clip, barrel_spawn_y, .. } = spec.controller
            else { panic!("False Knight runtime with other metadata") };
        if boss.controller.phase() == fk::Phase::Dormant {
            // `Detect`: the hero's collider entered the Battle Scene trigger.
            if overlap(trigger, context.hero_body) {
                let started = boss.arena.hero_entered();
                // `Detect` arms once; a hero who walks back through the curtain
                // gets an empty list rather than a second count.
                if !started.is_empty() { unsafe { HK_FK_TRIGGERED = HK_FK_TRIGGERED.saturating_add(1) } }
                self.apply_arena(boss, started);
            }
            return;
        }
        let arena = boss.arena.tick();
        self.apply_arena(boss, arena);
        // The Head has a HealthManager of its own, so it has its own 0.15 s
        // invulnerability window; the caller only ticks the body's.
        boss.head.tick();
        boss.animation_tick = boss.animation_tick.saturating_add(1);
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params {
            speed: boss.vx.abs(),
            // SetGravity2dScale against the installed Physics2DSettings gravity.
            gravity: (((60 * ONE) as i64 * boss.gravity as i64) >> 16) as i32,
            fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO
        };
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        if !boss.separated {
            // `Start Fall` turns gravity on while the body is still inside the
            // ceiling slab it is authored in. The source body is not solved
            // against terrain; this one is, so it is separated once here, with
            // the same resolution every other actor spawns through.
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            boss.separated = true;
            self.spawn_state = SpawnState::Ready;
            unsafe { HK_FK_DROPPED = HK_FK_DROPPED.saturating_add(1) }
        }
        if !boss.kinematic {
            body.step(p, boss.vx.signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        self.vy = body.vy;
        self.grounded = body.grounded;
        let phase = boss.controller.phase();
        // Only the states that read a sense pay for it. `Walls Check` is a
        // one-shot state the source enters out of `Determine Jump`, which only
        // `Move Choice` reaches and only `Idle` reaches that; `JA Fall` is the
        // one place the 9.5-unit fall ray is read; and `GetDistance` is read by
        // `Move Choice` and by `Run`. Casting all three every tick would put
        // three passes over the view's terrain on a stall-bound frame.
        let idle = phase == fk::Phase::Idle;
        let falling = matches!(phase, fk::Phase::JumpAttackAir | fk::Phase::DeathAir);
        let distance = if idle || phase == fk::Phase::Run {
            distance_q16(context.hero[0] - self.x, context.hero[1] - self.y)
        } else { 0 };
        let senses = fk::Senses {
            self_x: self.x, hero_x: context.hero[0], distance,
            velocity_y: body.vy, velocity_x: boss.vx, grounded: body.grounded,
            wall_left: idle && segment_hits_terrain([self.x, self.y],
                [self.x - fk::WALL_RAY_DISTANCE, self.y], count, edge),
            wall_right: idle && segment_hits_terrain([self.x, self.y],
                [self.x + fk::WALL_RAY_DISTANCE, self.y], count, edge),
            ground_below: falling && segment_hits_terrain([self.x, self.y],
                [self.x, self.y - fk::FALL_RAY_DISTANCE], count, edge),
        };
        let actions = boss.controller.tick(senses);
        self.apply_false_knight(boss, actions);
        // The one exit that reaches `FK Barrel Summon`. The other two write the
        // boss back on a nail hit and on the arena's own countdown, and neither
        // source path sends SUMMON from either of those.
        if boss.summon != 0 {
            let spawns = core::mem::take(&mut boss.summon);
            emit(RunnerEvent { scene: self.scene, source_id: self.source_id,
                position: [self.x, self.y], facing: self.walk.direction,
                kind: RunnerEventKind::Summon { spawns, spawn_y: barrel_spawn_y, barrel_clip } });
        }
    }
    /// One 60 Hz step of the Baldur: FSM tick from last frame's contacts, then
    /// the gravity body (scale 0.8) moves at the held x velocity and the
    /// Recoil displacement, and this frame's wall/ground contacts are kept.
    fn advance_baldur(&mut self, roller: &mut BaldurRuntime, spec: &ActorSpec, count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        use hk_sim::baldur::Action;
        use hk_sim::runner_senses as q;
        roller.animation_tick = roller.animation_tick.saturating_add(1);
        let b = spec.bounds;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let pos = [self.x, self.y];
        // Alert Range New: 21.14 x 1.9 trigger box at y +0.31 against the hero body.
        let hb = context.hero_body;
        let alert = [pos[0] - 692715, pos[1] - 41943, pos[0] + 692715, pos[1] + 82575];
        let in_alert_range = alert[0] <= hb[2] && alert[2] >= hb[0] && alert[1] <= hb[3] && alert[3] >= hb[1];
        let can_see_hero = match q::line_of_sight(pos, context.hero, in_alert_range, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Baldur LOS outside validated coordinates"),
        };
        let senses = hk_sim::baldur::Senses { actor_x: self.x, hero_x: context.hero[0], can_see_hero, wall: roller.wall, grounded: self.grounded };
        let actions = roller.controller.tick(senses);
        for action in actions.iter() {
            match action {
                Action::VelocityX(v) => roller.vx = v,
                Action::Velocity(v) => { roller.vx = v[0]; self.vy = v[1]; self.grounded = false; }
                Action::Play(clip) => { roller.clip = clip; roller.animation_tick = 0; }
                Action::Facing(f) => self.walk.direction = f,
            }
        }
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        let mut p = Params { speed: roller.vx.abs(), gravity: 48 * ONE, fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        if self.spawn_state == SpawnState::Pending {
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) { self.spawn_state = SpawnState::Blocked; return; }
            self.spawn_state = SpawnState::Ready;
        }
        let old_x = body.x;
        body.step(p, roller.vx.signum(), false, count, edge);
        roller.wall = roller.vx != 0 && body.x != old_x + roller.vx / 60;
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            p.gravity = 0;
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.vy = body.vy;
        self.grounded = body.grounded;
        self.x = body.x - offset;
        self.y = body.y;
    }
    /// One 60 Hz step of the Aspid: senses (alert circle plus sight, unalert
    /// circle, clear ray), FSM tick, gravity-free body against terrain, recoil.
    fn advance_aspid(&mut self, aspid: &mut AspidRuntime, spec: &ActorSpec, count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::aspid::Action;
        use hk_sim::runner_senses as q;
        aspid.animation_tick = aspid.animation_tick.saturating_add(1);
        let pos = [self.x, self.y];
        let hb = context.hero_body;
        let dx = (pos[0].clamp(hb[0], hb[2]) - pos[0]) as i64;
        let dy = (pos[1].clamp(hb[1], hb[3]) - pos[1]) as i64;
        const ALERT: i64 = 511463; // 7.804
        const UNALERT: i64 = 792986; // 12.1
        let in_alert = dx * dx + dy * dy <= ALERT * ALERT;
        let in_unalert = dx * dx + dy * dy <= UNALERT * UNALERT;
        let sight = |gate: bool| match q::line_of_sight(pos, context.hero, gate, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Aspid LOS outside validated coordinates"),
        };
        let clear = sight(true);
        let senses = hk_sim::aspid::Senses { position: pos, hero: context.hero, can_see_hero: in_alert && clear,
            in_unalert_range: in_unalert && clear, sight_clear: clear };
        let actions = aspid.controller.tick(senses);
        let ActorController::Aspid { shot_clip, impact_clip, .. } = spec.controller else { panic!("Aspid runtime with other metadata") };
        for action in actions.iter() {
            match action {
                Action::Play(clip, tick) => { aspid.clip = clip; aspid.animation_tick = tick; }
                Action::Facing(f) => self.walk.direction = f,
                Action::Fire(velocity) => emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: pos,
                    facing: self.walk.direction, kind: RunnerEventKind::Fire { velocity, shot_clip, impact_clip } }),
                Action::Velocity(_) => {}
            }
        }
        let b = spec.bounds;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let v = aspid.controller.velocity();
        let mut body = Player::spawn(self.x + offset, self.y);
        let mut p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        body.vy = v[1];
        body.step(p, v[0].signum(), false, count, edge);
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
    }
    fn runner_senses(&self, runner:&RunnerRuntime, spec:&ActorSpec, room:&Room,
        context:RunnerContext, count:usize, edge:impl Fn(usize)->[i32;4]) -> hk_sim::runner::Senses {
        use hk_sim::runner_senses as q;
        let pos = [self.x,self.y];
        let ActorController::Runner { alert, .. } = spec.controller else { panic!("Runner senses without Runner metadata") };
        // Body and alert boxes come from the placement (facing-left frame); the
        // level37 Runner shape is `Shape::RUNNER`.
        let shape = q::Shape::from_boxes(spec.bounds, alert);
        let expected = q::body_bounds_of(shape, pos, runner.controller.facing()).expect("Runner bounds");
        assert_eq!(self.bounds(spec), expected, "unsupported Runner collider variant");
        let in_alert_range = q::alert_overlap_of(shape,pos,context.hero_body).expect("Runner alert bounds");
        let can_see_hero = match q::line_of_sight(pos,context.hero,in_alert_range,count,&edge) {
            Ok(value) => value,
            // Coincident transform points have no proven Physics2D equivalent;
            // conservatively suppress sight (TOOK DAMAGE still reacts).
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Runner LOS outside validated coordinates"),
        };
        let probes = q::walker_queries_of(shape,pos,runner.controller.facing(),count,edge)
            .expect("Runner Sweep outside validated coordinates");
        hk_sim::runner::Senses {
            camera_in_start_range: hk_sim::runner::camera_in_start_range(context.camera,[self.x,self.y,66]),
            hero_x:context.hero[0], actor_x:self.x, in_alert_range, can_see_hero,
            wall:probes.wall, floor_ahead:probes.floor_ahead,
            completed:runner.completed(spec,room), grounded:self.grounded,
        }
    }
    fn emit_runner(&self, kind:RunnerEventKind, emit:&mut impl FnMut(RunnerEvent)) {
        emit(RunnerEvent { scene:self.scene,source_id:self.source_id,
            position:[self.x,self.y],facing:self.walk.direction,kind });
    }
    fn apply_runner_actions(&mut self, runner:&mut RunnerRuntime, actions:hk_sim::runner::Actions,
        emit:&mut impl FnMut(RunnerEvent)) {
        use hk_sim::runner::Action;
        self.walk.direction = runner.controller.facing();
        for action in actions.iter() {
            let kind = match action {
                Action::Play(_) => { runner.animation_tick=0; continue; }
                Action::Velocity { x,y } => {
                    if let Some(x)=x {runner.vx=x;}
                    if let Some(y)=y {self.vy=y;}
                    continue;
                }
                Action::AudioStop => RunnerEventKind::AudioStop,
                Action::AudioPlay => RunnerEventKind::AudioPlay,
                Action::ChaseSound => {
                    // Preserve the first overwritten pitch draw, then the actual
                    // .85..1.15 pitch and equal-weight clip selection. This is a
                    // deterministic per-actor RNG, not Unity global RNG parity.
                    let _overwritten_pitch = RunnerRuntime::draw(&mut runner.rng);
                    let pitch_q16 = 55706 + (RunnerRuntime::draw(&mut runner.rng) % 19661) as i32;
                    let variant = (RunnerRuntime::draw(&mut runner.rng) >> 31) as u8;
                    RunnerEventKind::ChaseSound { pitch_q16,variant }
                }
                Action::DustStart => RunnerEventKind::DustStart,
                Action::DustStop => RunnerEventKind::DustStop,
                Action::Turn(_) => continue, // controller/clip/facing own the turn
            };
            self.emit_runner(kind,emit);
        }
    }

}
#[derive(Clone, Copy, Debug)]
pub struct Events {
    pub hits: u16,
    pub kills: u16,
    pub hurt: Hurt,
    /// SOUL taken by a Dream Nail slash this tick, summed over the actors hit.
    pub dream_soul: u16,
}
pub struct EnemyWorld {
    actors: [Option<Actor>; MAX_ACTORS],
    shots: [Option<Shot>; SHOTS],
    /// `FK Barrel Summon`'s own `summon`, which is a second object in the room
    /// rather than part of the boss: `FalseyControl` writes `Spawns` and sends
    /// SUMMON, and this counts the burst out at its own pace. It lives here
    /// beside the pool it fills rather than on the boss runtime, where the
    /// `Runtime` enum would have paid for it in all thirty-two actor slots.
    summon: hk_sim::false_knight::Summon,
    /// The scene, drop height and cooked clip the live summoner uses, taken
    /// from the boss's `ActorSpec` when SUMMON arrives. `None` until a fight
    /// asks for a barrel, which is what keeps every other room's tick at one
    /// test of a `u8`.
    summon_site: Option<(u8, i32, u16)>,
}
impl EnemyWorld {
    /// Source shots are pooled per Aspid (two each); the oldest live shot
    /// yields when the shared pool is full.
    fn spawn_shot(&mut self, scene: usize, position: [i32; 2], velocity: [i32; 2], shot_clip: u16, impact_clip: u16) {
        self.spawn_projectile(Shot { scene: scene as u8, x: position[0], y: position[1], vx: velocity[0],
            vy: velocity[1], animation_tick: 0, impact: false, barrel: false, shot_clip, impact_clip });
    }
    /// `summon`'s `Spawn`: one `Falling Barrel` at the chosen x and the
    /// summoner's own y, at rest, with the pool's own recycling rule.
    fn spawn_barrel(&mut self, scene: usize, position: [i32; 2], clip: u16) {
        self.spawn_projectile(Shot { scene: scene as u8, x: position[0], y: position[1], vx: 0, vy: 0,
            animation_tick: 0, impact: false, barrel: true, shot_clip: clip, impact_clip: clip });
    }
    fn spawn_projectile(&mut self, shot: Shot) {
        let slot = self.shots.iter().position(|s| s.is_none())
            .unwrap_or_else(|| { let mut oldest = 0; for (i, s) in self.shots.iter().enumerate() {
                if s.map_or(0, |s| s.animation_tick) >= self.shots[oldest].map_or(0, |s| s.animation_tick) { oldest = i; } } oldest });
        self.shots[slot] = Some(shot);
    }
    /// EnemyBullet: gravity .05 flight, Collision on terrain or the HeroBox
    /// (DamageHero 1), Impact clip, then recycled. Off-scene shots are dropped.
    ///
    /// A barrel walks the same path with `Falling Barrel`'s own gravity and box.
    /// Its collider is a trigger, so terrain ends it rather than stopping it,
    /// which is what `Idle`'s `Trigger2dEventLayer` on layer 8 does; and it has
    /// no Impact clip, because `Break` turns the sprite off and everything else
    /// that state does is particles and a one-shot. So a broken barrel leaves
    /// at once rather than holding a slot through a frame it cannot draw.
    fn tick_shots(&mut self, region: &Region, hero_body: [i32; 4], count: usize, edge: &impl Fn(usize) -> [i32; 4],
        mut hurt: impl FnMut(u16, i32) -> Hurt) -> Hurt {
        let mut result = Hurt::Ignored;
        for slot in self.shots.iter_mut() {
            let Some(shot) = slot else { continue };
            if shot.scene as usize != region.scene { continue; }
            shot.animation_tick = shot.animation_tick.saturating_add(1);
            if shot.impact {
                if shot.animation_tick >= IMPACT_TICKS { *slot = None; }
                continue;
            }
            shot.vy -= shot.gravity() / 60;
            let (ox, oy) = (shot.x, shot.y);
            shot.x += shot.vx / 60;
            shot.y += shot.vy / 60;
            if !crate::world::contains(region.collision_bounds, shot.x, shot.y) && !crate::world::contains(region.bounds, shot.x, shot.y) {
                if shot.x.abs() > 512 * ONE || shot.y.abs() > 512 * ONE { *slot = None; continue; }
            }
            let hit_terrain = segment_hits_terrain([ox, oy], [shot.x, shot.y], count, edge);
            let half = shot.half();
            let bounds = [shot.x - half[0], shot.y - half[1], shot.x + half[0], shot.y + half[1]];
            let hit_hero = overlap(bounds, hero_body);
            if hit_hero {
                // A barrel falls straight down, so the recoil side is the side
                // of the hero it landed on rather than the side it came from.
                let outcome = hurt(shot.damage(), if shot.barrel {
                    if shot.x < (hero_body[0] + hero_body[2]) / 2 { -1 } else { 1 }
                } else if shot.vx < 0 { -1 } else { 1 });
                if outcome != Hurt::Ignored { result = outcome; }
            }
            if hit_terrain || hit_hero {
                if shot.barrel {
                    unsafe { HK_FK_BARRELS_BROKEN = HK_FK_BARRELS_BROKEN.saturating_add(1) }
                    *slot = None;
                    continue;
                }
                shot.impact = true;
                shot.animation_tick = 0;
                shot.vx = 0; shot.vy = 0;
            }
        }
        result
    }
    pub fn clear_shots(&mut self, scene: usize) {
        for slot in self.shots.iter_mut() {
            if slot.is_some_and(|s| s.scene as usize == scene) { *slot = None; }
        }
        // A scene load recreates `FK Barrel Summon` at its own start state, so
        // a burst the player left mid-count does not resume on the way back in.
        if self.summon_site.is_some_and(|(id, _, _)| id as usize == scene) {
            self.summon = hk_sim::false_knight::Summon::new(scene as u32);
            self.summon_site = None;
        }
    }
    /// Drain each source actor's death event once, even across local views.
    pub fn take_geo_deaths(&mut self,scene:usize,mut emit:impl FnMut(u32,i32,i32)->bool) {
        for actor in self.actors.iter_mut().flatten().filter(|a|a.scene==scene) {
            if actor.health.dead && !actor.geo_paid {
                actor.geo_paid=emit(actor.source_id,actor.x,actor.y);
            }
        }
    }
    pub const fn new() -> Self {
        Self {
            actors: [None; MAX_ACTORS],
            shots: [None; SHOTS],
            summon: hk_sim::false_knight::Summon::new(1),
            summon_site: None,
        }
    }
    /// Compact read-only state for route validation/telemetry.
    pub fn actor_state(&self, scene: usize, source_id: u32) -> Option<(i32, i32, i16)> {
        self.actors
            .iter()
            .flatten()
            .find(|a| a.scene == scene && a.source_id == source_id)
            .map(|a| (a.x, a.y, a.health.hp))
    }
    pub fn reset_scene(&mut self, scene: usize) {
        for slot in &mut self.actors {
            if slot.as_ref().is_some_and(|a| a.scene == scene) {
                *slot = None;
            }
        }
        self.clear_shots(scene);
    }
    pub fn sync_region(&mut self, region: &Region) {
        for spec in crate::world::region_actors(region) {
            if self
                .actors
                .iter()
                .flatten()
                .any(|a| a.scene == region.scene && a.source_id == spec.source_id)
            {
                continue;
            }
            let slot = self
                .actors
                .iter_mut()
                .find(|a| a.is_none())
                .expect("enemy source pool exceeds 32");
            *slot = Some(Actor::new(region.scene, spec));
            // A boss reports its load-time state before anything advances it,
            // so a route that never crosses the trigger still reads 65 hp and a
            // dormant fight rather than the zeros a fresh session starts with.
            let seated = slot.as_ref().expect("the actor just seated");
            if let Some(boss) = seated.false_knight() { seated.publish_false_knight(boss); }
        }
    }
    /// Call only on a running 60 Hz simulation tick, after Knight movement/nail
    /// state and before applying transitions. The caller owns hit-stop timing.
    pub fn tick<'a>(
        &mut self,
        region: &Region,
        room: &Room,
        state: &mut State,
        player: &mut Player,
        vitals: &mut Vitals,
        nail: &Nail,
        dream: &hk_sim::DreamNail,
        // Vengeful Spirit's world box while one is in flight, and its damage.
        ball: Option<([i32; 4], u16)>,
        response: &mut hk_sim::NailResponse,
        attack: AttackParams,
        polygons: [&[[i32; 2]]; 4],
        cheats: crate::cheats::Settings,
        camera: [i32;3],
        mut runner_event: impl FnMut(RunnerEvent),
        resident: impl Fn(usize, i32, i32) -> Option<(Region, Room<'a>)>,
    ) -> Events {
        self.sync_region(region);
        let mut events = Events {
            hits: 0,
            kills: 0,
            hurt: Hurt::Ignored,
            dream_soul: 0,
        };
        // The Dream Nail carries its own polygon, live for the whole slash.
        let mut dream_points = [[0; 2]; 16];
        let dream_polygon: &[[i32; 2]] = if dream.hitting() {
            let src = crate::DREAM_NAIL_POLYGON;
            for (dst, p) in dream_points.iter_mut().zip(src) {
                *dst = [player.x - p[0] * player.facing, player.y + p[1]];
            }
            &dream_points[..src.len()]
        } else {
            &dream_points[..0]
        };
        let mut points = [[0; 2]; 16];
        let polygon = if nail.hitting(attack) {
            let src = polygons[nail.kind as usize];
            assert!((3..=16).contains(&src.len()));
            for (dst, p) in points.iter_mut().zip(src) {
                *dst = [player.x - p[0] * player.facing, player.y + p[1]];
            }
            &points[..src.len()]
        } else {
            &points[..0]
        };
        let body = [
            player.x - crate::PARAMS.half_width,
            player.y + crate::PARAMS.bottom,
            player.x + crate::PARAMS.half_width,
            player.y + crate::PARAMS.top,
        ];
        let context=RunnerContext {camera,hero:[player.x,player.y],hero_body:body};
        // Aspid shots spawn after the actor pass (the pool is a sibling field).
        let mut fires: [Option<(usize, [i32; 2], [i32; 2], u16, u16)>; 4] = [None; 4];
        // At most one SUMMON a tick: one boss, and `Actions` carries one of
        // these per state change.
        let mut summoned: Option<(usize, u8, i32, u16)> = None;
        let mut runner_event = |event: RunnerEvent| {
            if let RunnerEventKind::Fire { velocity, shot_clip, impact_clip } = event.kind {
                if let Some(slot) = fires.iter_mut().find(|f| f.is_none()) {
                    *slot = Some((event.scene, event.position, velocity, shot_clip, impact_clip));
                }
            } else if let RunnerEventKind::Summon { spawns, spawn_y, barrel_clip } = event.kind {
                summoned = Some((event.scene, spawns, spawn_y, barrel_clip));
            } else { runner_event(event); }
        };
        for actor in self
            .actors
            .iter_mut()
            .flatten()
            .filter(|a| a.scene == region.scene)
        {
            let Some(spec) = crate::world::region_actors(region)
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            // WalkLeftRight keeps running independently of the Knight/camera.
            // A cooked view boundary is not a source scene or FSM boundary.
            let (x,y)=actor.corpse.map_or((actor.x,actor.y),|c|(c.x,c.y));
            let other=if crate::world::contains(region.bounds,x,y) {None}
                else {resident(region.scene,x,y)};
            let (terrain_region,terrain_room)=other.as_ref()
                .map_or((region,room),|(r,room)|(r,room));
            // ponytail: with a whole scene resident, only actors within one view of
            // the active view advance; farther ones hold their state (source Unity
            // keeps simulating them). Widen or drop when a scene needs it.
            let b=region.bounds;
            let near=crate::world::contains([b[0]-NEAR[0],b[1]-NEAR[1],b[2]+NEAR[0],b[3]+NEAR[1]],x,y);
            let available=near && terrain_region.scene==region.scene
                && crate::world::contains(terrain_region.collision_bounds,x,y);
            if !actor.health.dead {actor.health.tick();}
            if available {
                actor.advance_in_view(spec,state,region,room,terrain_region,terrain_room,context,&mut runner_event);
            }
            if actor.health.dead {continue;}
            if actor.runner().is_some() && (!available || actor.spawn_state==SpawnState::Blocked) {
                continue; // no attack/recoil sensing against an unavailable terrain owner
            }
            let bounds = actor.bounds(spec);
            // The boss answers the nail through two HealthManagers rather than
            // one and hurts the hero from the Hitter as well as its own body,
            // so it does not take the shared Runner/flyer path below.
            if let Some(mut boss) = actor.false_knight() {
                let mut reached = false;
                if let Some((box_, damage)) = ball {
                    let (_, hit) = actor.strike_false_knight(&mut boss, spec, |b| overlap(box_, b), damage);
                    if matches!(hit, Hit::Damaged | Hit::Killed) { events.hits += 1; }
                }
                if !polygon.is_empty() {
                    let nail_damage = cheats.params(crate::VITAL_PARAMS).nail_damage;
                    let (hit_box, hit) = actor.strike_false_knight(&mut boss, spec,
                        |b| hk_sim::polygon_hits_box(polygon, b), nail_damage);
                    reached = hit_box;
                    if matches!(hit, Hit::Damaged | Hit::Killed) {
                        events.hits += 1;
                        vitals.gain_soul_on_nail_hit(cheats.params(crate::VITAL_PARAMS));
                    }
                }
                if reached && vitals.can_control() {
                    response.contact(crate::NAIL_RESPONSE_PARAMS, nail.kind, player.facing, player);
                }
                if !dream_polygon.is_empty() && spec.dream_soul != 0 && !actor.dream_taken
                    && hk_sim::polygon_hits_box(dream_polygon, bounds) {
                    actor.dream_taken = true;
                    events.dream_soul = events.dream_soul.saturating_add(spec.dream_soul);
                }
                // The body's own DamageHero, which the stagger switches off, and
                // the `Hitter` trigger the attacks switch on, which reaches
                // nearly eight units ahead of the body the source never moves.
                let mut damage = 0;
                if boss.contact_damage != 0 && overlap(body, bounds) { damage = boss.contact_damage; }
                if boss.hitter && overlap(body, boss.hitter_box(actor.x, actor.y)) {
                    damage = hk_sim::false_knight::CONTACT_DAMAGE;
                }
                if damage != 0 {
                    let direction = if player.x < actor.x { -1 } else { 1 };
                    let hurt = cheats.hurt(vitals, crate::VITAL_PARAMS, damage, direction, false, player.shadow_dashing);
                    if hurt != Hurt::Ignored { events.hurt = hurt; }
                }
                actor.seat_false_knight(boss);
                continue;
            }
            // A fireball passes through what it hits: Fireball Control only
            // listens for the terrain layer, so the enemy's own invulnerability
            // window is what stops it hitting twice.
            if let Some((box_, damage)) = ball {
                if overlap(box_, bounds) {
                    match actor.health.hit(spec.health, damage) {
                        Hit::Damaged => events.hits += 1,
                        Hit::Killed => {
                            events.hits += 1;
                            events.kills += 1;
                        }
                        _ => {}
                    }
                }
            }
            if !dream_polygon.is_empty() && spec.dream_soul != 0 && !actor.dream_taken
                && hk_sim::polygon_hits_box(dream_polygon, bounds) {
                actor.dream_taken = true;
                events.dream_soul = events.dream_soul.saturating_add(spec.dream_soul);
            }
            if !polygon.is_empty() && hk_sim::polygon_hits_box(polygon, bounds) {
                if vitals.can_control() {
                    response.contact(
                        crate::NAIL_RESPONSE_PARAMS,
                        nail.kind,
                        player.facing,
                        player,
                    );
                }
                match actor
                    .health
                    .hit(spec.health, cheats.params(crate::VITAL_PARAMS).nail_damage)
                {
                    Hit::Damaged | Hit::Killed => {
                        events.hits += 1;
                        if actor.health.dead {
                            events.kills += 1;
                            if let Some(runner)=actor.runner_mut() {
                                runner.controller.die();
                                actor.emit_runner(RunnerEventKind::Destroy,&mut runner_event);
                            }
                            match &mut actor.runtime {
                                Runtime::Climber(c)=>c.controller.die(),
                                Runtime::Vengefly(f)=>f.controller.die(),
                                Runtime::Gruzzer(f)=>f.controller.die(),
                                Runtime::Baldur(r)=>r.controller.die(),
                                Runtime::Aspid(a)=>a.controller.die(),
                                // The False Knight answers the nail through its
                                // own branch above and never reaches this one.
                                Runtime::Walker|Runtime::Runner(_)|Runtime::Static {..}|Runtime::FalseKnight(_)=>{}
                            }
                            state.enemy_death_particles(region,actor.source_id,[actor.x,actor.y,0]);
                            if let Some(corpse_spec)=spec.corpse {
                                let mut corpse=hk_sim::Corpse::spawn(corpse_spec,actor.x,actor.y,nail.kind,
                                    player.facing,actor.source_id);
                                // EmitCorpse chooses world X sign from the hit cardinal,
                                // cancelling the parent actor sign for horizontal strikes.
                                corpse.facing=if nail.kind<2 {-player.facing}else{actor.walk.direction};
                                actor.corpse=Some(corpse);
                            }
                        }
                        // Through the live parameter stack, like the damage
                        // above it: SoulGain adds Soul Catcher and Soul Eater
                        // to the same 11 the cooked base carries.
                        vitals.gain_soul_on_nail_hit(cheats.params(crate::VITAL_PARAMS));
                        // HealthManager TOOK DAMAGE precedes Recoil's global
                        // horizontal event. RecoilByDirection ignores a request
                        // while already recoiling; accepted damage still reacts.
                        let begin_recoil = actor.runner().is_none() || actor.recoil_left == 0;
                        if !actor.health.dead {
                            if let Some(mut runner)=actor.runner() {
                                let actions=runner.controller.took_damage(player.x,actor.x);
                                actor.apply_runner_actions(&mut runner,actions,&mut runner_event);
                                if nail.kind<2 && begin_recoil {
                                    let senses=actor.runner_senses(&runner,spec,room,context,
                                        terrain_room.counts[5],|i|state.edge_in_view(region,room,terrain_region,terrain_room,i));
                                    let actions=runner.controller.horizontal_recoil(senses,|w,e|RunnerRuntime::choose(&mut runner.rng,w,e));
                                    actor.apply_runner_actions(&mut runner,actions,&mut runner_event);
                                }
                                actor.runtime=Runtime::Runner(runner);
                            }
                        }
                        if let Some(mut fly)=actor.vengefly() {
                            if !actor.health.dead {
                                let senses=actor.vengefly_senses(context,terrain_room.counts[5],
                                    &|i|state.edge_in_view(region,room,terrain_region,terrain_room,i));
                                let actions=fly.controller.took_damage(senses);
                                fly.apply(actions);
                            }
                            actor.runtime=Runtime::Vengefly(fly);
                        }
                        let dead=actor.health.dead;
                        if let Some(climber)=actor.climber_mut() {
                            // Recoil.OnHandleFreeze: freeze in place with the Stun
                            // clip; no displacement recoil for this kinematic body.
                            if !dead {
                                let actions=climber.controller.freeze();
                                for action in actions.iter() {
                                    match action {
                                        hk_sim::climber::Action::Velocity(v)=>climber.velocity=v,
                                        hk_sim::climber::Action::Play(clip)=>{climber.clip=clip;climber.animation_tick=0;}
                                        _=>{}
                                    }
                                }
                            }
                        } else if begin_recoil {
                        if let (Some(roller),true)=(actor.baldur_mut(),nail.kind<2 && !dead) {
                            // Recoil's global RECOIL HORIZONTAL: Recoil Decel resets the roll speed.
                            let actions=roller.controller.horizontal_recoil();
                            for action in actions.iter() {
                                match action {
                                    hk_sim::baldur::Action::VelocityX(v)=>roller.vx=v,
                                    hk_sim::baldur::Action::Play(clip)=>{roller.clip=clip;roller.animation_tick=0;}
                                    _=>{}
                                }
                            }
                        }
                        actor.recoil_left = spec.recoil_ticks;
                        actor.recoil = match nail.kind {
                            2 => [0, spec.recoil_speed],
                            3 => [0, -spec.recoil_speed],
                            _ => [player.facing * spec.recoil_speed, 0],
                        };
                        }
                    }
                    _ => {}
                }
            }
            let damage = actor.health.contact_damage(spec.health);
            if damage != 0 && overlap(body, bounds) {
                let direction = if player.x < actor.x { -1 } else { 1 };
                let hurt = cheats.hurt(vitals,crate::VITAL_PARAMS, damage, direction, false, player.shadow_dashing);
                if hurt != Hurt::Ignored {
                    events.hurt = hurt;
                }
            }
        }
        for (scene, position, velocity, shot_clip, impact_clip) in fires.into_iter().flatten() {
            self.spawn_shot(scene, position, velocity, shot_clip, impact_clip);
        }
        if let Some((scene, spawns, spawn_y, barrel_clip)) = summoned {
            self.summon_site = Some((scene as u8, spawn_y, barrel_clip));
            self.summon.summon(spawns);
        }
        // `Determine Spawns` / `Spawn` / `Check Spawns`: one barrel a frame at
        // most, and one `u8` test a frame while the summoner is idle, which is
        // every frame of every other room in the game.
        if let Some((scene, spawn_y, clip)) = self.summon_site {
            if let Some(x) = self.summon.tick() {
                self.spawn_barrel(scene as usize, [x, spawn_y], clip);
                unsafe { HK_FK_BARRELS = HK_FK_BARRELS.saturating_add(1) }
            }
        }
        let shadow = player.shadow_dashing;
        let hurt = self.tick_shots(region, body, room.counts[5], &|i| state.edge(region, room, i),
            |damage, direction| cheats.hurt(vitals, crate::VITAL_PARAMS, damage, direction, false, shadow));
        if hurt != Hurt::Ignored { events.hurt = hurt; }
        events
    }
    /// Create the immutable visible working set before preparing the GPU cache.
    pub fn prepare_draws(&self, region: &Region, room: &Room, camera: (i32, i32)) -> Draws {
        let mut draws = Draws {
            entries: [None; MAX_VISIBLE],
            len: 0,
        };
        for actor in self
            .actors
            .iter()
            .flatten()
            .filter(|a| a.scene == region.scene)
        {
            let Some(spec) = crate::world::region_actors(region)
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            let (clip_id,animation_tick,position,facing)=if actor.health.dead {
                let (Some(corpse),Some(corpse_spec))=(actor.corpse,spec.corpse) else {continue;};
                if !corpse.visible(){continue;}
                (corpse.clip(corpse_spec),corpse.animation_tick,(corpse.x,corpse.y),corpse.facing)
            } else if let Some(runner)=actor.runner() {
                (runner.clip(spec),runner.animation_tick,(actor.x,actor.y),runner.controller.facing())
            } else if let Some(climber)=actor.climber() {
                (climber.clip(spec),climber.animation_tick,(actor.x,actor.y),1)
            } else if let Some(fly)=actor.vengefly() {
                // The source sprite faces left (-1 native, +1 mirrored), as the Crawler.
                (fly.clip(spec),fly.animation_tick,(actor.x,actor.y),fly.controller.facing())
            } else if let Some(fly)=actor.gruzzer() {
                (spec.walk_clip,fly.animation_tick,(actor.x,actor.y),fly.controller.facing())
            } else if let Some(roller)=actor.baldur() {
                (roller.clip(spec),roller.animation_tick,(actor.x,actor.y),roller.controller.facing())
            } else if let Some(aspid)=actor.aspid() {
                (aspid.clip(spec),aspid.animation_tick,(actor.x,actor.y),aspid.controller.facing())
            } else if let Some(boss)=actor.false_knight() {
                // `Dormant` hangs the body inside the ceiling slab above the
                // arena, where the room's own scenery covers it in the source.
                // Actor draws here are not depth sorted against scenery, so the
                // closer answer is to draw nothing until it drops.
                if boss.controller.phase()==hk_sim::false_knight::Phase::Dormant {continue;}
                (boss.clip(spec),boss.animation_tick,(actor.x,actor.y),boss.facing())
            } else if let Some(animation_tick)=actor.immobile() {
                let ActorController::Static {idle_clip}=spec.controller else {panic!("Static runtime with other metadata")};
                (idle_clip,animation_tick,(actor.x,actor.y),actor.walk.direction)
            } else {
                (if actor.walk.turn_remaining!=0 {spec.turn_clip}else{spec.walk_clip},
                    actor.walk.animation_tick,(actor.x,actor.y),actor.walk.direction)
            };
            assert!((clip_id as usize) < room.counts[4]);
            let clip = room.clip(clip_id as usize);
            let elapsed = (animation_tick as u64 * clip[2] as u64 / (60 * 65536)) as u32;
            let mode = clip[3] & 65535;
            let start = clip[3] >> 16;
            let frame = if mode == 0 {
                elapsed % clip[1]
            } else if mode == 1 && elapsed >= clip[1] {
                start + (elapsed - start) % (clip[1] - start)
            } else {
                elapsed.min(clip[1] - 1)
            };
            let frame_index = (clip[0] + frame) as usize;
            let f = room.frame(frame_index);
            let base = u32_at(f, 0) as usize;
            // A live Climber's sprite turns with its transform (Q16 degrees).
            let rotation = actor.climber().filter(|_| !actor.health.dead).map(|c| c.controller.rotation()).unwrap_or(0);
            let project = |b: [i32; 4]| {
                let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
                let mut verts = [(0i16, 0i16); 4];
                for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
                    let [x, y] = if rotation != 0 { crate::world::debris::rotate([x * facing, y], rotation) } else { [x * facing, y] };
                    let x = position.0 - x - camera.0;
                    let y = position.1 + y - camera.1;
                    *v = (
                        (160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                        (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                    );
                }
                verts
            };
            // An actor frame past one 64x64 animation slot binds a rectangle of
            // them, the way NPC art already does, and each tile is its own quad
            // over its own share of the frame's world box. The grid rides in the
            // frame's first texture, so an actor whose art fits one slot pays
            // one texture read and never enters the tile arithmetic.
            let (cols, rows) = room.texture(base).tile_grid();
            for tile in 0..cols * rows {
                let (texture, b) = if cols * rows == 1 {
                    (base, core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4)))
                } else {
                    room.frame_tile(frame_index, tile)
                };
                let verts = project(b);
                // Culled per tile, so an actor leaning out of the view spends
                // neither a quad nor an animation slot on the part off screen.
                if verts.iter().all(|v| v.0 < 0)
                    || verts.iter().all(|v| v.0 >= 320)
                    || verts.iter().all(|v| v.1 < 0)
                    || verts.iter().all(|v| v.1 >= 240)
                {
                    continue;
                }
                assert!(
                    draws.len < MAX_VISIBLE,
                    "visible enemy cache budget exceeded"
                );
                draws.entries[draws.len] = Some(Draw {
                    texture: texture as u16,
                    verts,
                });
                draws.len += 1;
            }
        }
        for shot in self.shots.iter().flatten().filter(|s| s.scene as usize == region.scene) {
            let clip_id = if shot.impact { shot.impact_clip } else { shot.shot_clip } as usize;
            assert!(clip_id < room.counts[4]);
            let clip = room.clip(clip_id);
            let elapsed = (shot.animation_tick as u64 * clip[2] as u64 / (60 * 65536)) as u32;
            let frame = if clip[3] & 65535 == 0 { elapsed % clip[1] } else { elapsed.min(clip[1] - 1) };
            let f = room.frame((clip[0] + frame) as usize);
            let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
            let facing = if shot.vx > 0 { 1 } else { -1 };
            let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
            let mut verts = [(0i16, 0i16); 4];
            for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
                let x = shot.x - x * facing - camera.0;
                let y = shot.y + y - camera.1;
                *v = ((160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                      (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16);
            }
            if verts.iter().all(|v| v.0 < 0) || verts.iter().all(|v| v.0 >= 320) || verts.iter().all(|v| v.1 < 0) || verts.iter().all(|v| v.1 >= 240) { continue; }
            if draws.len >= MAX_VISIBLE { break; }
            draws.entries[draws.len] = Some(Draw { texture: u32_at(f, 0) as u16, verts });
            draws.len += 1;
        }
        draws
    }
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
#[derive(Clone, Copy)]
struct Draw {
    texture: u16,
    verts: [(i16, i16); 4],
}
pub struct Draws {
    entries: [Option<Draw>; MAX_VISIBLE],
    len: usize,
}
impl Draws {
    pub fn append_needed(&self, needed: &mut [u16], len: &mut usize) {
        for entry in self.entries.iter().flatten() {
            if needed[..*len].contains(&entry.texture) {
                continue;
            }
            assert!(*len < needed.len(), "animation working set exceeded");
            needed[*len] = entry.texture;
            *len += 1;
        }
    }
    pub fn draw(&self) -> u32 {
        for entry in self.entries.iter().flatten() {
            crate::render::texture(entry.texture as usize, entry.verts, (128, 128, 128));
        }
        self.len as u32
    }
}

/// Call before world::State::strike so a prop's first destruction hit can still
/// bounce. Already-destroyed colliders are rejected by their stable source state.
pub fn pogo_contact(
    region: &Region,
    state: &State,
    player: &mut Player,
    nail: &Nail,
    attack: AttackParams,
    polygons: [&[[i32; 2]]; 4],
    response: &mut hk_sim::NailResponse,
) -> bool {
    if !nail.hitting(attack) {
        return false;
    }
    let src = polygons[nail.kind as usize];
    assert!((3..=16).contains(&src.len()));
    let mut points = [[0; 2]; 16];
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for (dst, p) in points.iter_mut().zip(src) {
        *dst = [player.x - p[0] * player.facing, player.y + p[1]];
        bounds[0] = bounds[0].min(dst[0]);
        bounds[1] = bounds[1].min(dst[1]);
        bounds[2] = bounds[2].max(dst[0]);
        bounds[3] = bounds[3].max(dst[1]);
    }
    for target in crate::world::pogo_targets(region) {
        if nail.kind != 3 && !target.horizontal_and_up() {
            continue;
        }
        if target.breakable().is_some_and(|id| state.broken(id)) {
            continue;
        }
        if overlap(bounds, target.bounds()) && target.hit_by(&points[..src.len()]) {
            response.contact(
                crate::NAIL_RESPONSE_PARAMS,
                nail.kind,
                player.facing,
                player,
            );
            return true;
        }
    }
    false
}

#[cfg(test)]
mod runner_runtime_tests {
    use super::*;
    use hk_sim::runner::{Swipe, Walker};
    const SPEC: ActorSpec = ActorSpec {
        controller: ActorController::Runner {
            idle_clip: 2, anticipate_clip: 3, lunge_clip: 4, cooldown_clip: 5,
            params: hk_sim::runner::Params::RUNNER, alert: hk_sim::runner_senses::ALERT_LOCAL,
        },
        source_id: 5196, x: 0, y: 94208,
        bounds: [-51200,-94208,28672,12288],
        health: hk_sim::EnemyParams {health:15,contact_damage:1,evasion_ticks:12,
            invincible:false,damage_override:false},
        walk:hk_sim::WalkParams {speed:ONE+ONE/2,turn_ticks:10,turn_cooldown_ticks:60},
        initial_direction:-1,random_start_direction:false,walk_clip:0,turn_clip:1,
        corpse:Some(hk_sim::CorpseSpec {air_clip:2,land_clip:2,bounds:[-ONE/2,-ONE,ONE/2,0],
            spawn_offset:[0,ONE/2],bounce_factor:13107, fling_speed: 15 * hk_sim::ONE, gravity: 48 * hk_sim::ONE, breaker: false, smash_bounces: 0, remove_after_land: 0, hold_ticks: 0}),
        recoil_speed:10*ONE,recoil_ticks:9, dream_soul: 0
    };
    // Source-free six-clip bank with the real Runner rates/lengths. Each clip's
    // frames reference a distinct texture so draw selection is independently visible.
    fn bytes() -> Vec<u8> {
        let lengths=[7u32,2,6,5,8,1];
        let mut b=Vec::from(*b"HKROOM02");
        for n in [1,6,0,29,6,1,0,0] {b.extend((n as u32).to_le_bytes());}
        for _ in 0..6 {for n in [0u16,0,0,4,4,0] {b.extend(n.to_le_bytes());}
            b.extend(0u32.to_le_bytes());}
        for (clip,len) in lengths.iter().enumerate() {
            for _ in 0..*len {for n in [clip as i32,-ONE/2,-ONE,ONE/2,0] {b.extend(n.to_le_bytes());}}
        }
        let mut first=0;
        for (i,len) in lengths.iter().enumerate() {
            let rate=if i==0||i==2 {10}else{12};
            let wrap=if i==0||i==2 {0}else{2};
            for n in [first,*len,rate*65536,wrap] {b.extend(n.to_le_bytes());}
            first+=len;
        }
        for n in [-100*ONE,0,100*ONE,0] {b.extend(n.to_le_bytes());}
        b.resize(b.len()+6*32+32768,0); b
    }
    fn context(hero_x:i32) -> RunnerContext {
        let y=SPEC.y;
        RunnerContext {camera:[0,2*ONE,-2496922],hero:[hero_x,y],
            hero_body:[hero_x-crate::PARAMS.half_width,y+crate::PARAMS.bottom,
                hero_x+crate::PARAMS.half_width,y+crate::PARAMS.top]}
    }
    fn advance(actor:&mut Actor,room:&Room,c:RunnerContext,events:&mut Vec<RunnerEvent>) {
        actor.advance(&SPEC,room,room.counts[5],|i|room.edge(i),c,&mut |e|events.push(e));
    }
    fn phase(actor:&Actor)->Swipe {actor.runner().unwrap().controller.swipe()}
    fn region()->Region {
        Region {scene:0,bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],
            collision_bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],actors:&[&SPEC]}
    }
    #[test]
    fn actual_runner_clock_motion_clips_and_ordered_effects() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut actor=Actor::new(0,&SPEC);let mut events=Vec::new();let c=context(3*ONE);
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Anticipate);
        assert_eq!(actor.walk.direction,1);
        assert_eq!(actor.runner().unwrap().clip(&SPEC),3);
        assert_eq!(events.iter().map(|e|e.kind).take(3).collect::<Vec<_>>(),
            [RunnerEventKind::AudioStop,RunnerEventKind::AudioPlay,RunnerEventKind::AudioStop]);
        assert!(matches!(events[3].kind,RunnerEventKind::ChaseSound {pitch_q16:55706..=75366,variant:0..=1}));
        for _ in 0..24 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Anticipate);
        let before=actor.x;
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Lunge);
        assert_eq!(actor.runner().unwrap().clip(&SPEC),4);
        for _ in 0..39 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Lunge);
        assert_eq!(actor.x-before,40*(6*ONE/60));
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Cooldown);
        let stopped=actor.x;
        for _ in 0..5 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Idle);
        assert_eq!(actor.x,stopped);
        let mut far=context(30*ONE);far.camera=[0,2*ONE,-2496922];
        for _ in 0..15 {advance(&mut actor,&room,far,&mut events);}
        assert_eq!(phase(&actor),Swipe::Ready);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::DustStart).count(),1);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::DustStop).count(),1);
        let mut world=EnemyWorld::new();world.actors[0]=Some(actor);
        let draws=world.prepare_draws(&region(),&room,(actor.x,actor.y));
        assert_eq!(draws.entries[0].unwrap().texture,0); // actual Walk clip
        assert!(core::mem::size_of::<RunnerRuntime>()<=80);
        // One runtime enum per actor: the pool costs the largest controller.
        assert!(core::mem::size_of::<Actor>()<=256);
        println!("RunnerRuntime={} Actor={} EnemyWorld={}",core::mem::size_of::<RunnerRuntime>(),
            core::mem::size_of::<Actor>(),core::mem::size_of::<EnemyWorld>());
    }
    #[test]
    fn true_camera_depth_gates_start_and_live_terrain_blocks_los() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut actor=Actor::new(0,&SPEC);let mut events=Vec::new();
        let mut c=context(30*ONE);c.camera=[50*ONE,SPEC.y,-40*ONE];
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Waiting);
        assert_eq!(actor.x,SPEC.x);assert!(events.is_empty());
        c.camera=[0,SPEC.y,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        let before=actor.x;
        for _ in 0..40 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        assert!(actor.x<before);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        c.camera=[200*ONE,200*ONE,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        let actor=Actor::new(0,&SPEC);let runner=actor.runner().unwrap();
        let floor=room.edge(0);let wall=[ONE,0,ONE,5*ONE];
        let blocked=actor.runner_senses(&runner,&SPEC,&room,context(3*ONE),2,
            |i|if i==0 {floor}else{wall});
        assert!(blocked.in_alert_range);assert!(!blocked.can_see_hero);
        let clear=actor.runner_senses(&runner,&SPEC,&room,context(3*ONE),2,
            |i|if i==0 {floor}else{[0;4]});
        assert!(clear.can_see_hero); // destroyed/disabled live terrain sentinel
    }
    #[test]
    fn accepted_hits_reset_in_order_and_death_owns_single_corpse_geo_event() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,SPEC.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO };
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();let mut state=State;let mut response=hk_sim::NailResponse::new();
        let cheats=crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()};
        // Coincident origin has conservatively unknown LOS. Accepted damage must
        // nevertheless enter anticipation BEFORE its horizontal reset restarts Walk.
        world.sync_region(&r);nail.active=true;nail.age=1;
        let hit=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
            attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!((hit.hits,hit.kills),(1,0));
        assert_eq!(world.actors[0].unwrap().health.hp,10);
        let chase=events.iter().position(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})).unwrap();
        assert!(events[chase+1..].iter().any(|e|e.kind==RunnerEventKind::AudioPlay));
        assert_eq!(world.actors[0].unwrap().recoil_left,9);
        let after_first=events.len();
        world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
            attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!(world.actors[0].unwrap().health.hp,10); // evaded overlap
        assert!(!events[after_first..].iter().any(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})));
        // Isolate two further accepted health events from traversal/recoil timing.
        for expected in [5,0] {
            let actor=world.actors[0].as_mut().unwrap();
            actor.health.evasion_ticks=0;actor.recoil_left=0;player.x=actor.x;player.y=actor.y;
            let result=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
                attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
            assert_eq!(world.actors[0].unwrap().health.hp,expected);
            assert_eq!(result.kills,u16::from(expected==0));
        }
        let actor=world.actors[0].unwrap();assert!(actor.corpse.is_some());
        assert_eq!(phase(&actor),Swipe::Dead);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::Destroy).count(),1);
        let mut paid=Vec::new();
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        assert_eq!(paid,[SPEC.source_id]);
        assert_eq!(vitals.soul,33);
    }
    #[test]
    fn source_seed_and_effect_pitch_sequence_are_deterministic() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut a=Actor::new(0,&SPEC);let mut b=Actor::new(0,&SPEC);
        let mut ea=Vec::new();let mut eb=Vec::new();
        for _ in 0..180 {
            advance(&mut a,&room,context(3*ONE),&mut ea);
            advance(&mut b,&room,context(3*ONE),&mut eb);
            assert_eq!((a.x,a.y,a.walk.direction),(b.x,b.y,b.walk.direction));
        }
        assert_eq!(ea,eb);
        assert_eq!(a.runner().unwrap().rng,b.runner().unwrap().rng);
        assert!(ea.iter().any(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})));
    }
    #[test]
    fn runner_vertical_hit_does_not_emit_horizontal_restart() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,SPEC.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        nail.active=true;nail.age=1;nail.kind=2;
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO };
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();
        let hit=world.tick(&r,&room,&mut State,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,
            &mut hk_sim::NailResponse::new(),attack,[poly;4],
            crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()},
            [0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!(hit.hits,1);
        let actor=world.actors[0].unwrap();
        assert_eq!(phase(&actor),Swipe::Anticipate);
        assert_eq!(actor.recoil,[0,10*ONE]);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::AudioPlay).count(),1);
    }
    #[test]
    fn unavailable_runner_terrain_suspends_clock_and_resumes_same_actor() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let missing=Region {bounds:[110*ONE,0,120*ONE,5*ONE],
            collision_bounds:[110*ONE,0,120*ONE,5*ONE],..region()};
        let mut world=EnemyWorld::new();let mut events=Vec::new();
        let tick=|world:&mut EnemyWorld,r:&Region,events:&mut Vec<RunnerEvent>| {
            world.tick(r,&room,&mut State,&mut Player::spawn(30*ONE,SPEC.y),
                &mut Vitals::new(crate::VITAL_PARAMS),&Nail::new(),&hk_sim::DreamNail::new(),None,
                &mut hk_sim::NailResponse::new(),
                AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO },
                [&[];4],crate::cheats::Settings::new(),[0,SPEC.y,-2496922],
                |e|events.push(e),|_,_,_|None);
        };
        tick(&mut world,&r,&mut events);
        let before=world.actors[0].unwrap();events.clear();
        for _ in 0..120 {tick(&mut world,&missing,&mut events);}
        let suspended=world.actors[0].unwrap();
        assert_eq!((before.x,before.y),(suspended.x,suspended.y));
        assert_eq!(before.runner().unwrap().animation_tick,suspended.runner().unwrap().animation_tick);
        assert_eq!(before.runner().unwrap().rng,suspended.runner().unwrap().rng);
        assert!(events.is_empty());
        tick(&mut world,&r,&mut events);
        let resumed=world.actors[0].unwrap();
        assert_eq!(resumed.source_id,before.source_id);
        assert_eq!(resumed.runner().unwrap().animation_tick,before.runner().unwrap().animation_tick+1);
    }

}
