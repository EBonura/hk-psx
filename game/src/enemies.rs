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
    /// Aspid FireAtTarget, or the Blocker's `SpawnObjectFromGlobalPool`: one
    /// pooled projectile with this velocity, at the event position offset by
    /// `offset` (the Blocker's `Shot Origin`; zero for the Aspid, which spawns
    /// on its own transform). `goop` picks `Shot Mawlek`'s gravity over the
    /// Aspid bullet's.
    Fire { velocity: [i32; 2], offset: [i32; 2], goop: bool, shot_clip: u16, impact_clip: u16 },
    /// The Hatcher's `Fire`: wake one of the scene's parked cage members at the
    /// event position with this velocity. The event carries no identity for the
    /// baby because the source picks one at random from whatever is still in
    /// the cage; `EnemyWorld` owns the reservation and answers it.
    Release { velocity: [i32; 2] },
    /// The Blocker's `Fire` from its `Roller` branch: a `Spawn Roller v2` at
    /// the event position offset by `offset`, flung with `velocity`, rolling
    /// the way `facing` says once it lands (game/src/blocker_roller.rs).
    Roller { velocity: [i32; 2], offset: [i32; 2] },
    /// `SUMMON` to `FK Barrel Summon`, with `Spawns` already written. The
    /// summoner is a different source object from the boss and keeps its own
    /// clock, so this only hands over the count and where the barrels come
    /// from; `EnemyWorld` owns the loop that spaces them out.
    Summon { spawns: u8, spawn_y: i32, barrel_clip: u16 },
    /// `S Attack Recover`'s `Shockwave Wave`, spawned at the event position
    /// and travelling the way `facing` says (+1 right). `EnemyWorld` owns it,
    /// beside the barrels, because it outlives the state that made it.
    Shockwave,
    /// The Husk Guard's stomp: the same pooled wave at its own speed and
    /// scale (`hk_sim::husk_guard::WAVE`), drawing `spurt_clip` from its room.
    GuardWave { spurt_clip: u16 },
    /// A pooled one-shot sprite (the Husk Guard's `Slam Effect R`) played once
    /// where it lands, facing `facing`; it neither moves nor hurts.
    Effect { clip: u16 },
    /// Brooding Mawlek's FlingObjectsFromGlobalPool: `count` `Shot Mawlek
    /// NoDrip` from the event position, each drawn from `seed` with
    /// `hk_sim::mawlek::shot_velocity` inside these ranges.
    MawlekShots { count: u8, speed: [i32; 2], angles: [i32; 2], seed: u32 },
    /// Gruz Mother's burster `Spawn Flies 2`: `Fly Spawn` moves to the event
    /// position, which releases every parked reserve fly of the scene there.
    ReleaseReserve,
}
/// A resident sink for the events a strike cannot emit. Two code modules
/// passing their own empty closures got them folded into one body by the
/// linker's identical-code folding, and a module may not reach into another.
fn no_runner_event(_: RunnerEvent) {}
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
    /// `GetChildCount(Cage)` for the scene being ticked: how many Hatcher
    /// Babies are still parked. Every Hatcher in a scene reads the one cage, so
    /// this falls as the frame's releases are taken and a second Hatcher on the
    /// same frame sees what the first one left.
    cage_children: u16,
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
    /// The `Head` child's clip and its clock. It is drawn only while the
    /// controller says it is visible; the source parks it at y 150 otherwise.
    head_clip: hk_sim::false_knight::HeadClip,
    head_tick: u16,
    /// `Blow` has run: the body shows the empty armour from here on.
    blown: bool,
    /// The `Death Head`: 0 not out, 1 sliding out of the armour at `Death Head
    /// Speed` while `Death Head 1` plays, 2 stopped against a wall with that
    /// clip still playing, 3 landed on `Death Head 2`. It leaves on the side
    /// the body faces from `FK_DEATH_HEAD_OFFSET`, so how far it has slid is
    /// all of its position that is not the body's own. Kept this narrow
    /// because the widest `Runtime` variant is paid thirty-two times.
    death_head: u8,
    death_head_tick: u16,
    death_head_travel: i32,
}
impl FalseKnightRuntime {
    /// The sprite the body shows now, in host/false_knight_art.py's bank:
    /// every clip `FalseyControl` plays is cooked, so a body clip is its own
    /// art, and after `Blow` the body is the empty armour.
    fn body_sprite(&self) -> usize {
        if self.blown { return crate::fk_art::last_sprite(crate::fk_art::BODY); }
        crate::fk_art::sprite(self.clip as usize, self.animation_tick)
    }
    fn head_sprite(&self) -> Option<usize> {
        use hk_sim::false_knight::HeadClip;
        if !self.controller.head_visible() { return None; }
        let clip = match self.head_clip {
            HeadClip::Idle => crate::fk_art::HEAD_IDLE,
            HeadClip::Hit => crate::fk_art::HEAD_HIT,
            HeadClip::Spaz => crate::fk_art::HEAD_SPAZ,
        };
        Some(crate::fk_art::sprite(clip, self.head_tick as u32))
    }
    fn death_head_sprite(&self) -> Option<usize> {
        match self.death_head {
            1 | 2 => Some(crate::fk_art::sprite(crate::fk_art::DEATH_HEAD_1, self.death_head_tick as u32)),
            3 => Some(crate::fk_art::sprite(crate::fk_art::DEATH_HEAD_2, self.death_head_tick as u32)),
            _ => None,
        }
    }
    /// The Death Head's transform, from the body's and how far it slid.
    fn death_head_at(&self, x: i32, y: i32) -> [i32; 2] {
        let sign = if self.controller.facing_right() { 1 } else { -1 };
        let [dx, dy] = crate::fk_art::FK_DEATH_HEAD_OFFSET;
        [x + sign * dx + self.death_head_travel, y + dy]
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
/// Brooding Mawlek: `hk_sim::mawlek::Mawlek` is `Mawlek Control`, its Walker,
/// both `Mawlek Arm Control`s, `Mawlek Head` and the corpse's `corpse`, and
/// `hk_sim::boss::Arena` is the `Battle Scene`'s `Battle Control`, carried on
/// the boss for the False Knight's reason: it is the one thing the arena
/// counts. Lurking costs one AABB a tick and the Dummy's clip clock, no solver.
#[derive(Clone, Copy)]
struct MawlekRuntime {
    controller: hk_sim::mawlek::Mawlek,
    arena: hk_sim::boss::Arena,
    /// Clip and clock per part, in `MawlekRuntime::slot` order: the body, the
    /// Dummy, `Mawlek Arm R`, `Mawlek Arm L`, the Head.
    clips: [u8; 5],
    ticks: [u16; 5],
    /// Ticks since `Spit Effect` was told to PLAY; 0 while its mesh is off.
    spit: u8,
    vx: i32,
    /// SetGravity2dScale, Q16: 0 from `Init` until `Wake In Air`.
    gravity: i32,
    mesh: bool,
    /// `Start`'s SetInvincible false.
    vulnerable: bool,
    /// Each arm's swipe PolygonCollider2D, and whether this swing already
    /// clashed with the nail (`nail_clash_tink` answers once per swing).
    arm_hitbox: [bool; 2],
    parried: [bool; 2],
    /// The body (and later the corpse) was resolved out of the floor once.
    separated: bool,
    blown: bool,
}
impl MawlekRuntime {
    fn slot(part: hk_sim::mawlek::Part) -> usize {
        use hk_sim::mawlek::Part;
        match part { Part::Body => 0, Part::Dummy => 1, Part::Arm(arm) => 2 + arm as usize, Part::Head => 4 }
    }
    fn play(&mut self, part: hk_sim::mawlek::Part, clip: hk_sim::mawlek::Clip) {
        let slot = Self::slot(part);
        self.clips[slot] = clip as u8;
        self.ticks[slot] = 0;
    }
    /// The sprite a part shows now, if its clip has any frames.
    fn sprite(&self, slot: usize) -> Option<usize> {
        crate::mawlek_art::BANK.frame(self.clips[slot] as usize, self.ticks[slot] as u32)
    }
    fn relative(b: [i32; 4], x: i32, y: i32) -> [i32; 4] {
        [x + b[0], y + b[1], x + b[2], y + b[3]]
    }
}
/// Gruz Mother: `hk_sim::gruz_mother::GruzMother` is `Big Fly Control`, its
/// `bouncer_control`, the corpse's `corpse` and the burster's `burster`, and
/// `hk_sim::boss::Arena` is Crossroads_04's `Battle Control`, carried on the
/// boss for the False Knight's reason. Its seven `Battle Enemies` are the
/// reserve flies the burster releases, not the boss. Asleep it costs one
/// polygon test a tick and the clip clock, no solver.
#[derive(Clone, Copy)]
struct GruzRuntime {
    controller: hk_sim::gruz_mother::GruzMother,
    arena: hk_sim::boss::Arena,
    /// The clip clock, reset by every Play.
    tick: u16,
    /// The body (and later the burster) was resolved out of terrain once.
    separated: bool,
    /// CameraShake `RumblingMed`/`RumblingSmall` while on.
    rumble: bool,
    /// The burster's floor contact last step, for ObjectBounce's enter.
    grounded: bool,
    /// The last sprite whose tk2d definition wrote a box into the body's
    /// BoxCollider2D (an Unset sprite leaves it); NO_BOX until one has, which
    /// is the serialized collider (the body's, or the burster's once flung).
    box_sprite: u8,
}
const NO_BOX: u8 = u8::MAX;
impl GruzRuntime {
    /// The body box in the facing it has: the art and the cooked box are
    /// authored facing left, and SetScale -1.25 mirrors both.
    fn body(&self, b: [i32; 4]) -> [i32; 4] {
        if self.controller.facing_right() { [-b[2], b[1], -b[0], b[3]] } else { b }
    }
    /// The collider the current frame leaves on the object (tk2d writes a Box
    /// sprite's own box at every frame change), authored facing left.
    /// `serialized` is the prefab's box for before any sprite wrote one.
    /// Returns the box and whether it changed this tick.
    fn frame_box(&mut self, serialized: [i32; 4]) -> ([i32; 4], bool) {
        let table = &crate::gruz_art::GZ_SPRITE_BOX;
        let before = self.box_sprite;
        if let Some(sprite) = crate::gruz_art::BANK.frame(self.controller.clip() as usize, self.tick as u32) {
            if sprite < table.len() && !hk_sim::empty_edge(&table[sprite]) { self.box_sprite = sprite as u8; }
        }
        let b = if self.box_sprite == NO_BOX { serialized } else { table[self.box_sprite as usize] };
        (b, before != self.box_sprite)
    }
    fn facing(&self) -> i32 {
        if self.controller.facing_right() { 1 } else { -1 }
    }
}
/// The first hit woke it, BATTLE START sealed the arena.
#[no_mangle] pub static mut HK_GZ_WOKEN: u32 = 0;
/// Super attacks chosen, by kind, and wall or floor impacts.
#[no_mangle] pub static mut HK_GZ_CHARGES: u32 = 0;
#[no_mangle] pub static mut HK_GZ_SLAMS: u32 = 0;
#[no_mangle] pub static mut HK_GZ_IMPACTS: u32 = 0;
/// Nail or spell hits the body took, the HealthManager death and the cheat
/// bits live then, `Blow`, the burster's release and reserve flies killed.
#[no_mangle] pub static mut HK_GZ_HITS: u32 = 0;
#[no_mangle] pub static mut HK_GZ_DEATHS: u32 = 0;
#[no_mangle] pub static mut HK_GZ_KILL_CHEATS: u32 = 0;
#[no_mangle] pub static mut HK_GZ_BLOWN: u32 = 0;
#[no_mangle] pub static mut HK_GZ_RELEASED: u32 = 0;
#[no_mangle] pub static mut HK_GZ_FLY_DEATHS: u32 = 0;
/// 90 hp, the controller's phase (`hk_sim::gruz_mother::Phase` order), the
/// body's (or burster's) transform, and the arena's phase and `Activated`.
#[no_mangle] pub static mut HK_GZ_HP: i32 = 0;
#[no_mangle] pub static mut HK_GZ_PHASE: u32 = 0;
#[no_mangle] pub static mut HK_GZ_X: i32 = 0;
#[no_mangle] pub static mut HK_GZ_Y: i32 = 0;
#[no_mangle] pub static mut HK_GZ_ARENA: u32 = 0;
#[no_mangle] pub static mut HK_GZ_ACTIVATED: u32 = 0;
/// The burster's `Geo` state: 50 Geo Small to fling from here, which the
/// frame hands to the coin pool after the enemy pass (it owns `geo::World`).
static mut GZ_GEO: Option<(usize, [i32; 2])> = None;
pub fn take_burster_geo() -> Option<(usize, [i32; 2])> {
    unsafe { core::ptr::replace(core::ptr::addr_of_mut!(GZ_GEO), None) }
}
/// The body turned the corner out of `Dormant`: the hero crossed `Alert Range New`.
#[no_mangle] pub static mut HK_MW_WOKEN: u32 = 0;
/// The corpse's BATTLE END, the HealthManager death, and the cheats live then.
#[no_mangle] pub static mut HK_MW_DEATHS: u32 = 0;
#[no_mangle] pub static mut HK_MW_KILL_CHEATS: u32 = 0;
/// `Blow`: the corpse exploded, the end of everything the Mawlek draws.
#[no_mangle] pub static mut HK_MW_BLOWN: u32 = 0;
/// 300 hp, the controller's phase (`hk_sim::mawlek::Phase` order), out of
/// `Dormant` and alive, and the arena's phase and `Activated`.
#[no_mangle] pub static mut HK_MW_HP: i32 = 0;
/// The body's transform, and the corpse's once the body is gone.
#[no_mangle] pub static mut HK_MW_X: i32 = 0;
#[no_mangle] pub static mut HK_MW_Y: i32 = 0;
#[no_mangle] pub static mut HK_MW_PHASE: u32 = 0;
#[no_mangle] pub static mut HK_MW_ACTIVE: u32 = 0;
#[no_mangle] pub static mut HK_MW_ARENA: u32 = 0;
#[no_mangle] pub static mut HK_MW_ACTIVATED: u32 = 0;
/// Super attacks by kind, arm swipes, the nail clashing with a swipe, the
/// Head's single shots, and nail hits the body or the Head took.
#[no_mangle] pub static mut HK_MW_SPRAYS: u32 = 0;
#[no_mangle] pub static mut HK_MW_LEAPS: u32 = 0;
#[no_mangle] pub static mut HK_MW_SWIPES: u32 = 0;
#[no_mangle] pub static mut HK_MW_PARRIES: u32 = 0;
#[no_mangle] pub static mut HK_MW_HEAD_SHOTS: u32 = 0;
#[no_mangle] pub static mut HK_MW_HITS: u32 = 0;
/// Spit globs that reached the hero.
#[no_mangle] pub static mut HK_MW_SHOT_HITS: u32 = 0;
/// `Roar Lock`: the Knight takes no input while a boss roars at it.
static mut ROAR_LOCK: bool = false;
/// Between BATTLE END and `End Wait` the arena is won but its Heart Piece is
/// not out yet.
static mut MW_PIECE_WAIT: bool = false;
/// The pad a roar leaves the Knight: only Start (bit 3, `menu_state::START`),
/// so the game can still pause.
pub fn roar_lock(bits: u16) -> u16 {
    if unsafe { ROAR_LOCK } { bits & (1 << 3) } else { bits }
}
/// Whether `Battle Control` has activated Crossroads_09's Heart Piece: the
/// arena is won (its `Activated` is in the world store) and, in the visit that
/// won it, `End Wait` has run. A later visit's `Activate` shows it at once.
pub fn arena_piece_ready() -> bool {
    !unsafe { MW_PIECE_WAIT }
        && crate::persist::get(crate::persist::Kind::BattleScene, crate::mawlek_art::MW_SCENE, 0).is_some()
}
/// `host/cook.py` CAM_Z: the camera sits 38.1 units in front of the play plane,
/// which is what a lurking body 3.16 units behind it shrinks against.
const CAMERA_DISTANCE: i32 = 2496922;

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
/// The cheat bits live on the tick the boss died, so a route that uses a cheat
/// to travel can still prove the kill itself was fair.
#[no_mangle] pub static mut HK_FK_KILL_CHEATS: u32 = 0;
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
/// Nail hits the exposed Head took, the death exposure's included: the
/// maggot answering each one with `Head Hit` is what this counts.
#[no_mangle] pub static mut HK_FK_HEAD_HITS: u32 = 0;
/// `Shockwave Wave`s the slams spawned, and spurts that reached the hero.
#[no_mangle] pub static mut HK_FK_WAVES: u32 = 0;
#[no_mangle] pub static mut HK_FK_WAVE_HITS: u32 = 0;

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
/// `InfectedEnemyEffects.RecieveHitEffect` calls `SpriteFlash.flashInfected`:
/// colour (1, 0.31, 0) at amount 0.9, 0.01 s up, 0.01 s held, 0.25 s down.
/// At 60 Hz that is one tick at full strength and a 15-tick linear fade.
const FLASH_TICKS: u8 = 16;
/// The flash colour times its amount, as a GPU modulation of a white texel
/// (128 is 1.0): 0.9 * (1, 0.31, 0).
const FLASH_TINT: (u32, u32, u32) = (115, 36, 0);
fn flash_tint(left: u8) -> (u8, u8, u8) {
    let k = (left as u32).min(FLASH_TICKS as u32);
    let s = |c: u32| (c * k / FLASH_TICKS as u32) as u8;
    (s(FLASH_TINT.0), s(FLASH_TINT.1), s(FLASH_TINT.2))
}
/// Quads the enemy pass may submit. Larger than `MAX_VISIBLE` because a part
/// of the False Knight that lives in its scene's texture pages is a quad with
/// no animation key: only streamed draws count against the cache.
pub const MAX_DRAWS: usize = 48;
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
    /// One of Gruz Mother's reserve flies (`ActorController::GruzzerReserve`):
    /// parked below the room, no tick, draw or hit, until the burster's
    /// release; its death is reported to the arena once.
    reserve: bool,
    parked: bool,
    reported: bool,
}
/// Acid Flyer (Duranda): the controller owns the tween offset and the turn;
/// the body sits at the authored x and `origin_y` plus that offset, facing
/// through `walk.direction` so the shared box mirror applies.
#[derive(Clone, Copy)]
struct AcidFlyerRuntime {
    controller: hk_sim::acid_flyer::AcidFlyer,
    origin_y: i32,
    animation_tick: u32,
}
impl AcidFlyerRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        match self.controller.clip() {
            hk_sim::acid_flyer::Clip::Fly => spec.walk_clip,
            hk_sim::acid_flyer::Clip::TurnToFly => spec.turn_clip,
        }
    }
}
/// Mosquito: the controller owns velocity, facing, the lunge and its clips;
/// this runtime moves the body against terrain and reports the first contact
/// of a lunge one tick later, as OnCollisionEnter2D reaches the next Update.
#[derive(Clone, Copy)]
struct MosquitoRuntime {
    controller: hk_sim::mosquito::Mosquito,
    animation_tick: u32,
    hit_terrain: bool,
}
impl MosquitoRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::mosquito::Clip;
        let ActorController::Mosquito { clips, .. } = spec.controller else { panic!("Mosquito runtime with other metadata") };
        match self.controller.clip() {
            Clip::Idle => spec.walk_clip,
            Clip::TurnToIdle => spec.turn_clip,
            Clip::Startle => clips[0],
            Clip::AttackAntic => clips[1],
            Clip::Attack => clips[2],
            Clip::DeathAir => clips[3],
        }
    }
}
/// Moss Walker: the controller owns the burial, the walk velocity, the turns
/// and the clips; this runtime casts the rays it asks for and runs the
/// gravity body.
#[derive(Clone, Copy)]
struct MossWalkerRuntime {
    controller: hk_sim::moss_walker::MossWalker,
    animation_tick: u32,
}
impl MossWalkerRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        use hk_sim::moss_walker::Clip;
        let ActorController::MossWalker { clips } = spec.controller else { panic!("Moss Walker runtime with other metadata") };
        match self.controller.clip() {
            Clip::Walk => spec.walk_clip,
            Clip::Turn => spec.turn_clip,
            Clip::Rest => clips[0],
            Clip::Shake => clips[1],
            Clip::Appear => clips[2],
            Clip::Bury => clips[3],
        }
    }
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
/// Hatcher: the controller owns velocity, facing, clips and the release cue.
#[derive(Clone, Copy)]
struct HatcherRuntime {
    controller: hk_sim::hatcher::Hatcher,
    animation_tick: u32,
    clip: hk_sim::hatcher::Clip,
}
impl HatcherRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Hatcher { fire_clip, .. } = spec.controller
            else { panic!("Hatcher runtime with other metadata") };
        match self.clip { hk_sim::hatcher::Clip::Fly => spec.walk_clip, hk_sim::hatcher::Clip::Fire => fire_clip }
    }
}
/// Zombie Shield: the controller owns the Walker underneath it, the shield,
/// the two attack chains and every clip; this runtime runs the gravity body
/// and answers the nail through the controller's own direction guard.
#[derive(Clone, Copy)]
struct ZombieShieldRuntime {
    controller: hk_sim::zombie_shield::ZombieShield,
    rng: u32,
    animation_tick: u32,
}
impl ZombieShieldRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::ZombieShield { clips, .. } = spec.controller
            else { panic!("Zombie Shield runtime with other metadata") };
        match self.controller.clip().slot() {
            Some(slot) => clips[slot],
            None if self.controller.clip() == hk_sim::zombie_shield::Clip::Turn => spec.turn_clip,
            None => spec.walk_clip,
        }
    }
    fn completed(&self, spec: &ActorSpec, room: &Room) -> Option<hk_sim::zombie_shield::Animation> {
        let token = self.controller.animation()?;
        let id = self.clip(spec) as usize;
        assert!(id < room.counts[4], "Zombie Shield clip not resident");
        let clip = room.clip(id);
        assert!(clip[1] != 0 && clip[2] != 0, "empty Zombie Shield clip");
        // Only Once clips complete; the walk and idle loops never do, which is
        // what keeps the two attack chains on their own cooked frame counts.
        if clip[3] & 65535 == 2
            && self.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536 {
            Some(token)
        } else { None }
    }
    /// `Shield Start`'s `RandomInt(60, 100)`, inclusive at both ends, from the
    /// per-actor generator rather than a claim on Unity's global sequence.
    fn counter(rng: &mut u32, [hi, lo]: [u16; 2]) -> u16 {
        *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        lo + (*rng % (hi - lo + 1) as u32) as u16
    }
}
/// Husk Guard: the controller owns every velocity and clip; this runtime runs
/// the gravity body, the clip clock and the `Swipe` hitbox's life.
#[derive(Clone, Copy)]
struct HuskGuardRuntime {
    controller: hk_sim::husk_guard::HuskGuard,
    animation_tick: u32,
    /// Ticks the armed `Swipe` has left (`DeactivateAfter2dtkAnimation`).
    swipe_left: u16,
}
impl HuskGuardRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::HuskGuard { clips, .. } = spec.controller
            else { panic!("Husk Guard runtime with other metadata") };
        match self.controller.clip().slot() {
            Some(slot) => clips[slot],
            None if self.controller.clip() == hk_sim::husk_guard::Clip::Turn => spec.turn_clip,
            None => spec.walk_clip,
        }
    }
    fn completed(&self, spec: &ActorSpec, room: &Room) -> Option<hk_sim::husk_guard::Animation> {
        let token = self.controller.animation()?;
        let clip = room.clip(self.clip(spec) as usize);
        if clip[3] & 65535 == 2
            && self.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536 {
            Some(token)
        } else { None }
    }
}
/// Blocker: a turret with no body of its own. It never moves, never falls and
/// never recoils, so this runtime is the controller, its clip clock and the
/// generator the two `Random` draws come out of, and nothing else.
#[derive(Clone, Copy)]
struct BlockerRuntime {
    controller: hk_sim::blocker::Blocker,
    rng: u32,
    animation_tick: u32,
}
impl BlockerRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Blocker { clips, .. } = spec.controller
            else { panic!("Blocker runtime with other metadata") };
        match self.controller.clip().slot() {
            Some(slot) => clips[slot],
            None if self.controller.clip() == hk_sim::blocker::Clip::Closed => spec.turn_clip,
            None => spec.walk_clip,
        }
    }
    fn completed(&self, spec: &ActorSpec, room: &Room) -> Option<hk_sim::blocker::Animation> {
        let token = self.controller.animation()?;
        let id = self.clip(spec) as usize;
        assert!(id < room.counts[4], "Blocker clip not resident");
        let clip = room.clip(id);
        assert!(clip[1] != 0 && clip[2] != 0, "empty Blocker clip");
        // Only Once clips complete. Idle and Closed are the two loops, and both
        // are left on a sense rather than on a `Tk2dWatchAnimationEvents`.
        if clip[3] & 65535 == 2
            && self.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536 {
            Some(token)
        } else { None }
    }
    /// `WaitRandom(0.8, 1.2)` and `RandomFloat(X Speed Min, X Speed Max)`, both
    /// inclusive, from the per-actor generator rather than Unity's own.
    fn sample(rng: &mut u32, [lo, hi]: [i32; 2]) -> i32 {
        *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        lo + (*rng % (hi - lo + 1) as u32) as i32
    }
}
/// Greenpath Pigeon: a critter with one hit point, a trigger-only collider and
/// nothing to solve against. Perched it costs one circle test a tick and a
/// raycast only on the frames the hero is already inside the circle, which is
/// what the source's own trigger gates its raycast on.
#[derive(Clone, Copy)]
struct PigeonRuntime {
    controller: hk_sim::pigeon::Pigeon,
    animation_tick: u32,
}
impl PigeonRuntime {
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Pigeon { clips } = spec.controller
            else { panic!("Pigeon runtime with other metadata") };
        match self.controller.clip().slot() {
            Some(slot) => clips[slot],
            None if self.controller.clip() == hk_sim::pigeon::Clip::Fly => spec.turn_clip,
            None => spec.walk_clip,
        }
    }
}
/// One member of a Hatcher's cage. `Inert` is the parked state and costs one
/// test a tick: no senses, no body, no clip clock and no draw.
#[derive(Clone, Copy)]
struct BabyRuntime {
    controller: hk_sim::hatcher::Baby,
    animation_tick: u32,
}
/// Releases a scene may make on one frame. A Hatcher's own cycle is at least
/// 140 ticks, so this is only reachable with five Hatchers in one scene, and
/// the most any admitted scene has is the three in Crossroads_27. A Hatcher
/// that finds the buffer full is told the cage is empty, so it takes the
/// source's own `CANCEL` path back to Distance Fly instead of losing a spawn.
const RELEASES: usize = 4;
/// Source `Spitter Shot R`: a gravity 0.05 dynamic body that damages the hero
/// on contact and plays Impact where it hits terrain or the hero.
///
/// The False Knight's `Falling Barrel` rides the same pool. It is the same
/// shape of object: a trigger box on a dynamic body that damages the hero and
/// ends on terrain, and its own `PersonalObjectPool` reserve is also eight, so
/// sharing costs neither a second array nor a second integrate-and-collide
/// loop. Only one of the two can ever be live, because Crossroads_10 is the
/// only room with a boss and it carries no Aspid.
///
/// Brooding Mawlek's `Shot Mawlek NoDrip` rides it too: the Blocker's goop
/// again, 25 at once from the super spit plus the Head's one at a time. The
/// pool is 32 so a whole spray is on screen at once, as the source's global
/// pool has it, rather than the oldest eight; Crossroads_09 has no other
/// shooter.
pub const SHOTS: usize = 32;
const _: () = assert!(SHOTS >= hk_sim::false_knight::BARREL_POOL,
    "the shared projectile pool must hold FK Barrel Summon's whole reserve");
const SHOT_GRAVITY: i32 = 3 * ONE; // .05 * 60
/// The Blocker's `Shot Mawlek`: the same box and the same damage as the Aspid's
/// bullet, on twelve times the gravity, which is the whole of the difference
/// between a spit that flies flat and a goop that is lobbed over a gap.
const GOOP_GRAVITY: i32 = 36 * ONE; // .6 * 60
const SHOT_HALF: [i32; 2] = [20992, 18432]; // .640625 x .5625 box at scale .7 * .8... kept at source size
const IMPACT_TICKS: u32 = 18; // six frames at 20 fps
/// Which source prefab a pooled projectile is. All three are a box on a
/// dynamic body that damages the hero and ends on terrain; they differ in
/// gravity, and the barrel also in size, damage and having no Impact clip.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShotKind {
    /// `Spitter Shot R`, the Aspid's.
    Spit,
    /// `Shot Mawlek`, the Blocker's.
    Goop,
    /// `Falling Barrel`, `FK Barrel Summon`'s.
    Barrel,
    /// `Shot Mawlek NoDrip`, the Blocker's goop under another prefab, drawn
    /// from the Mawlek's own bank rather than the scene's clip table.
    Mawlek,
}
#[derive(Clone, Copy)]
struct Shot {
    scene: u8,
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    animation_tick: u32,
    impact: bool,
    kind: ShotKind,
    shot_clip: u16,
    impact_clip: u16,
}
impl Shot {
    fn gravity(&self) -> i32 {
        match self.kind {
            ShotKind::Spit => SHOT_GRAVITY,
            ShotKind::Goop | ShotKind::Mawlek => GOOP_GRAVITY,
            ShotKind::Barrel => hk_sim::false_knight::BARREL_GRAVITY,
        }
    }
    fn half(&self) -> [i32; 2] {
        if self.kind == ShotKind::Barrel { hk_sim::false_knight::BARREL_HALF } else { SHOT_HALF }
    }
    fn damage(&self) -> u16 {
        if self.kind == ShotKind::Barrel { hk_sim::false_knight::BARREL_DAMAGE } else { 1 }
    }
}
const WAVES: usize = 2;
/// Does the movement segment cross any terrain edge (closed endpoints)?
fn segment_hits_terrain(a: [i32; 2], b: [i32; 2], count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> bool {
    (0..count).any(|i| segment_crosses(a, b, edge(i)))
}
/// `segment_hits_terrain` over a copied edge list and the edges `mask` selects:
/// a crossing point is on the segment, so an edge whose x range misses the
/// segment's cannot cross it.
#[inline(never)]
fn segment_hits_edges(a: [i32; 2], b: [i32; 2], edges: &[[i32; 4]], mask: hk_sim::runner_senses::EdgeMask) -> bool {
    let mut hit = false;
    let _ = hk_sim::runner_senses::each_edge(edges.len(), mask, |i| {
        hit = hit || segment_crosses(a, b, edges[i]);
        Ok(())
    });
    hit
}
#[inline(never)]
pub(crate) fn segment_crosses(a: [i32; 2], b: [i32; 2], e: [i32; 4]) -> bool {
    // `e == [0; 4]` (removed terrain) as one compare instead of a memcmp call.
    if e[0] | e[1] | e[2] | e[3] == 0 { return false; }
    let (ox, oy) = (a[0] as i64, a[1] as i64);
    let (rx, ry) = (b[0] as i64 - ox, b[1] as i64 - oy);
    let (ax, ay) = (e[0] as i64, e[1] as i64);
    let (sx, sy) = (e[2] as i64 - ax, e[3] as i64 - ay);
    let den = rx * sy - ry * sx;
    if den == 0 { return false; }
    let (qx, qy) = (ax - ox, ay - oy);
    let (mut t, mut u, mut d) = (qx * sy - qy * sx, qx * ry - qy * rx, den);
    if d < 0 { t = -t; u = -u; d = -d; }
    t >= 0 && t <= d && u >= 0 && u <= d
}
/// `GetDistance` between two transforms, Q16: the exact floor root. World
/// coordinates stay inside +/-512 units, so the squares stay well below the
/// i64 range.
fn distance_q16(dx: i32, dy: i32) -> i32 {
    let square = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
    if square <= 0 { return 0; }
    psx_math::int32::isqrt_u64(square as u64) as i32
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
    // A hit point is on the ray, so only edges whose x range meets the ray's
    // can hit; the column index skips the rest, in the same index order.
    let bounds = [ox.min(end[0]).clamp(i32::MIN as i64, i32::MAX as i64) as i32, oy.min(end[1]).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        ox.max(end[0]).clamp(i32::MIN as i64, i32::MAX as i64) as i32, oy.max(end[1]).clamp(i32::MIN as i64, i32::MAX as i64) as i32];
    let _ = hk_sim::runner_senses::each_edge(count, edges_near(bounds), |i| {
        let e = edge(i);
        if e[0] | e[1] | e[2] | e[3] == 0 { return Ok(()); }
        let (ax, ay) = (e[0] as i64, e[1] as i64);
        let (sx, sy) = (e[2] as i64 - ax, e[3] as i64 - ay);
        let denominator = rx * sy - ry * sx;
        if denominator == 0 { return Ok(()); }
        // origin + t*r = a + u*s, both parameters in [0, 1], scaled by the denominator.
        let (qx, qy) = (ax - ox, ay - oy);
        let t_num = qx * sy - qy * sx;
        let u_num = qx * ry - qy * rx;
        let (t_num, u_num, den) = if denominator < 0 { (-t_num, -u_num, -denominator) } else { (t_num, u_num, denominator) };
        if t_num < 0 || t_num > den || u_num < 0 || u_num > den { return Ok(()); }
        // Compare the ray parameter as a fraction t_num/den across edges.
        let nearer = best.is_none_or(|(t, d, _): (i64, i64, [i32; 2])| t_num * d < t * den);
        if nearer {
            let point = [(ox + rx * t_num / den) as i32, (oy + ry * t_num / den) as i32];
            best = Some((t_num, den, point));
        }
        Ok(())
    });
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
    /// Each copy's column index (`EdgeColumns`), built on its first query
    /// after a fill, so copies no walker or Climber scans never pay for one.
    columns:[hk_sim::runner_senses::EdgeColumns;EDGE_SLOTS],
    indexed:[bool;EDGE_SLOTS],
    counts:[usize;EDGE_SLOTS],
    keys:[(usize,usize,u32);EDGE_SLOTS],
    next:usize,
}
#[cfg(not(test))]
static mut ACTOR_EDGES:ActorEdges=ActorEdges {edges:[[[0;4];EDGES];EDGE_SLOTS],
    columns:[hk_sim::runner_senses::EdgeColumns::ALL;EDGE_SLOTS],indexed:[false;EDGE_SLOTS],counts:[0;EDGE_SLOTS],
    keys:[(usize::MAX,0,0);EDGE_SLOTS],next:0};
/// The copy the actor being advanced reads its edges from, for the walker,
/// sight and Climber ray scans (`edges_near`). advance_in_view sets it for one
/// advance_body and resets it to `EDGE_SLOTS`, which selects every edge, so
/// any other edge source scans as before.
static mut ACTOR_SLOT:usize=EDGE_SLOTS;
#[inline(never)]
fn edges_near(bounds:[i32;4])->hk_sim::runner_senses::EdgeMask {
    #[cfg(not(test))]
    unsafe {
        let slot=ACTOR_SLOT;
        if slot<EDGE_SLOTS {
            // Field places only: advance_body holds a shared borrow of this
            // slot's edges, so no reference to the whole cache is made here.
            let cache=core::ptr::addr_of_mut!(ACTOR_EDGES);
            if !(*cache).indexed[slot] {
                let count=(*cache).counts[slot];
                let row=&*(&raw const (*cache).edges[slot]);
                (*cache).columns[slot]=hk_sim::runner_senses::EdgeColumns::build(&row[..count]);
                (*cache).indexed[slot]=true;
            }
            return (*cache).columns[slot].near(bounds);
        }
    }
    let _=bounds;
    hk_sim::runner_senses::ALL_EDGES
}
#[cfg(not(test))]
fn actor_edges<'a>(state:&State,active:&Region,active_room:&Room,target:&Region,target_room:&Room)->(&'a [[i32;4]],usize) {
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
            cache.indexed[slot]=false;cache.counts[slot]=count;
            cache.keys[slot]=key;
            slot
        }
    };
    (&cache.edges[slot][..count],slot)
}
#[derive(Clone, Copy)]
struct Actor {
    scene: usize,
    source_id: u32,
    x: i32,
    y: i32,
    vy: i32,
    grounded: bool,
    /// The placement's authored facing, which never changes: `local_bounds`
    /// mirrors the cooked box against it and the Blocker's sprite reads it.
    /// It is held here rather than looked up again because the placement lives
    /// in the scene bank while `ActorSpec` is the shared type.
    initial_direction: i8,
    /// A Climber placement's authored rotation in quarter turns, which the
    /// same box rotation is measured from.
    spawn_quarter: i8,
    spawn_state: SpawnState,
    health: ActorHealth,
    walk: WalkState,
    recoil_left: u16,
    recoil: [i32; 2],
    /// Ticks left of the hit flash (`SpriteFlash.flashInfected`), counted down
    /// once per tick; see `FLASH_TICKS`.
    flash_left: u8,
    corpse: Option<hk_sim::Corpse>,
    geo_paid: bool,
    /// A Dream Nail slash pays SOUL once per actor, as the source's
    /// `EnemyDreamnailReaction` clears its state after the first impact.
    dream_taken: bool,
    /// Where the off-view resolver last found this actor's view, and the box
    /// it holds for, so a lookup scans the bank only on leaving that box.
    located: crate::disc::Located,
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
    AcidFlyer(AcidFlyerRuntime),
    Mosquito(MosquitoRuntime),
    MossWalker(MossWalkerRuntime),
    Baldur(BaldurRuntime),
    Aspid(AspidRuntime),
    Hatcher(HatcherRuntime),
    HatcherBaby(BabyRuntime),
    ZombieShield(ZombieShieldRuntime),
    HuskGuard(HuskGuardRuntime),
    Blocker(BlockerRuntime),
    Pigeon(PigeonRuntime),
    /// No senses and no body: only the looping clip's clock advances.
    Static { animation_tick: u32 },
    FalseKnight(FalseKnightRuntime),
    Mawlek(MawlekRuntime),
    GruzMother(GruzRuntime),
}
// `EnemyWorld` is a field of `frame::Game`, which is a local of `main`, so the
// widest `Runtime` variant is paid thirty-two times inside main's stack frame.
// The Hatcher and its cage member are both far narrower than the boss; this
// holds the pair to that promise rather than leaving it to a later measurement.
// 92 since the boss carries its Head's clip and the Death Head's slide: eight
// bytes a slot, 256 bytes of main's frame in all.
const _: () = assert!(core::mem::size_of::<Runtime>() == 92,
    "a controller runtime wider than the False Knight's grows main's stack frame by 32 of the difference");
impl Actor {
    fn runner(&self) -> Option<RunnerRuntime> { if let Runtime::Runner(r) = self.runtime { Some(r) } else { None } }
    fn runner_mut(&mut self) -> Option<&mut RunnerRuntime> { if let Runtime::Runner(r) = &mut self.runtime { Some(r) } else { None } }
    fn climber(&self) -> Option<ClimberRuntime> { if let Runtime::Climber(c) = self.runtime { crate::modules::require(crate::modules::CLIMBER).then_some(c) } else { None } }
    fn climber_mut(&mut self) -> Option<&mut ClimberRuntime> { if let Runtime::Climber(c) = &mut self.runtime { crate::modules::require(crate::modules::CLIMBER).then_some(c) } else { None } }
    fn vengefly(&self) -> Option<VengeflyRuntime> { if let Runtime::Vengefly(v) = self.runtime { crate::modules::require(crate::modules::VENGEFLY).then_some(v) } else { None } }
    fn gruzzer(&self) -> Option<GruzzerRuntime> { if let Runtime::Gruzzer(g) = self.runtime { crate::modules::require(crate::modules::GRUZZER).then_some(g) } else { None } }
    fn acid_flyer(&self) -> Option<AcidFlyerRuntime> { if let Runtime::AcidFlyer(f) = self.runtime { crate::modules::require(crate::modules::ACID_FLYER).then_some(f) } else { None } }
    fn mosquito(&self) -> Option<MosquitoRuntime> { if let Runtime::Mosquito(m) = self.runtime { crate::modules::require(crate::modules::MOSQUITO).then_some(m) } else { None } }
    /// The Mosquito's `TileDetector` in the world while it still has one: a
    /// nail target and part of its terrain body, mirrored with its facing.
    fn mosquito_tile(&self, spec: &ActorSpec) -> Option<[i32; 4]> {
        let ActorController::Mosquito { tile, .. } = spec.controller else { return None };
        let Runtime::Mosquito(m) = self.runtime else { return None };
        if !m.controller.tile_detector() { return None; }
        let b = if self.walk.direction < 0 { tile } else { [-tile[2], tile[1], -tile[0], tile[3]] };
        Some([self.x + b[0], self.y + b[1], self.x + b[2], self.y + b[3]])
    }
    fn moss_walker(&self) -> Option<MossWalkerRuntime> { if let Runtime::MossWalker(m) = self.runtime { crate::modules::require(crate::modules::MOSS_WALKER).then_some(m) } else { None } }
    /// A buried Moss Walker: `SetInvincible` with preventInvincibleEffect (no
    /// recoil, no effect), NonBouncer active and DamageHero 0.
    fn buried(&self) -> bool {
        matches!(self.runtime, Runtime::MossWalker(m) if m.controller.hidden())
    }
    /// The detached `Shell`'s world box: it follows the body and never mirrors.
    fn acid_flyer_shell(&self, spec: &ActorSpec) -> Option<[i32; 4]> {
        let ActorController::AcidFlyer { shell, .. } = spec.controller else { return None };
        Some([self.x + shell[0], self.y + shell[1], self.x + shell[2], self.y + shell[3]])
    }
    fn baldur(&self) -> Option<BaldurRuntime> { if let Runtime::Baldur(b) = self.runtime { crate::modules::require(crate::modules::BALDUR).then_some(b) } else { None } }
    fn baldur_mut(&mut self) -> Option<&mut BaldurRuntime> { if let Runtime::Baldur(b) = &mut self.runtime { crate::modules::require(crate::modules::BALDUR).then_some(b) } else { None } }
    fn aspid(&self) -> Option<AspidRuntime> { if let Runtime::Aspid(a) = self.runtime { crate::modules::require(crate::modules::ASPID).then_some(a) } else { None } }
    fn hatcher(&self) -> Option<HatcherRuntime> { if let Runtime::Hatcher(h) = self.runtime { crate::modules::require(crate::modules::HATCHER).then_some(h) } else { None } }
    fn baby(&self) -> Option<BabyRuntime> { if let Runtime::HatcherBaby(b) = self.runtime { crate::modules::require(crate::modules::HATCHER).then_some(b) } else { None } }
    fn zombie_shield(&self) -> Option<ZombieShieldRuntime> {
        if let Runtime::ZombieShield(z) = self.runtime { crate::modules::require(crate::modules::HUSKS).then_some(z) } else { None }
    }
    fn blocker(&self) -> Option<BlockerRuntime> { if let Runtime::Blocker(b) = self.runtime { crate::modules::require(crate::modules::BLOCKER).then_some(b) } else { None } }
    fn husk_guard(&self) -> Option<HuskGuardRuntime> {
        if let Runtime::HuskGuard(g) = self.runtime { crate::modules::require(crate::modules::HUSKS).then_some(g) } else { None }
    }
    fn pigeon(&self) -> Option<PigeonRuntime> { if let Runtime::Pigeon(p) = self.runtime { crate::modules::require(crate::modules::PIGEON).then_some(p) } else { None } }
    /// A cage member still in the cage: `Control`'s `Inert`.
    fn parked_baby(&self) -> bool {
        self.baby().is_some_and(|b| b.controller.phase() == hk_sim::hatcher::BabyPhase::Inert)
    }
    /// The `Death` state, which recycles instead of removing: hp back to five,
    /// the dead flag cleared, the body reparented to the cage and moved to the
    /// cage's own origin, which is where the placement already puts it.
    ///
    /// `geo_paid` and `dream_taken` deliberately survive, because `Death` does
    /// not reset either source component: the baby drops no Geo at all, and its
    /// `EnemyDreamnailReaction` pays once and stays paid until the scene reloads.
    fn park_baby(&mut self, placement: &hk_sim::ActorPlacement, spec: &ActorSpec) {
        self.x = placement.x;
        self.y = placement.y;
        self.vy = 0;
        self.recoil_left = 0;
        self.recoil = [0; 2];
        self.health = ActorHealth::new(spec.health);
        if let Runtime::HatcherBaby(baby) = &mut self.runtime {
            baby.controller.park();
            baby.animation_tick = 0;
        }
    }
    fn immobile(&self) -> Option<u32> { if let Runtime::Static { animation_tick } = self.runtime { Some(animation_tick) } else { None } }
    // Boss and husk code streams with its rooms (modules.rs): these refuse a
    // runtime whose module is not resident, so it is inert rather than a call
    // into a trap.
    fn false_knight(&self) -> Option<FalseKnightRuntime> {
        if let Runtime::FalseKnight(b) = self.runtime { crate::modules::require(crate::modules::FALSE_KNIGHT).then_some(b) } else { None }
    }
    fn mawlek(&self) -> Option<MawlekRuntime> {
        if let Runtime::Mawlek(m) = self.runtime { crate::modules::require(crate::modules::MAWLEK).then_some(m) } else { None }
    }
    fn gruz(&self) -> Option<GruzRuntime> {
        if let Runtime::GruzMother(g) = self.runtime { crate::modules::require(crate::modules::GRUZ_MOTHER).then_some(g) } else { None }
    }
    /// A reserve fly still waiting under the room.
    fn parked_reserve(&self) -> bool {
        self.gruzzer().is_some_and(|f| f.parked)
    }
    fn seat_gruz(&mut self, boss: GruzRuntime) {
        self.runtime = Runtime::GruzMother(boss);
        if !boss.controller.asleep() {
            self.publish_gruz(boss);
        }
    }
    fn publish_gruz(&self, boss: GruzRuntime) {
        unsafe {
            HK_GZ_HP = self.health.hp as i32;
            HK_GZ_PHASE = boss.controller.phase() as u32;
            HK_GZ_X = self.x;
            HK_GZ_Y = self.y;
            HK_GZ_ARENA = boss.arena.phase() as u32;
            HK_GZ_ACTIVATED = u32::from(boss.arena.activated());
        }
    }
    /// Write the Mawlek back and mirror what a route reads off it, the False
    /// Knight's `seat_false_knight` rule: every exit goes through here, and a
    /// lurking body publishes nothing because nothing on it can move.
    fn seat_mawlek(&mut self, boss: MawlekRuntime) {
        self.runtime = Runtime::Mawlek(boss);
        if boss.controller.phase() != hk_sim::mawlek::Phase::Dormant {
            self.publish_mawlek(boss);
        }
    }
    fn publish_mawlek(&self, boss: MawlekRuntime) {
        unsafe {
            HK_MW_HP = self.health.hp as i32;
            HK_MW_X = self.x;
            HK_MW_Y = self.y;
            HK_MW_PHASE = boss.controller.phase() as u32;
            HK_MW_ACTIVE = u32::from(boss.controller.active());
            HK_MW_ARENA = boss.arena.phase() as u32;
            HK_MW_ACTIVATED = u32::from(boss.arena.activated());
        }
    }
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
    fn is_flyer(&self) -> bool { matches!(self.runtime, Runtime::Vengefly(_) | Runtime::Gruzzer(_) | Runtime::Baldur(_)
        | Runtime::Aspid(_) | Runtime::Hatcher(_) | Runtime::HatcherBaby(_)) }
    fn new(scene: usize, placement: &hk_sim::ActorPlacement, spec: &ActorSpec) -> Self {
        // Stable per-source seed, not a claim to reproduce Unity's global RNG.
        let seed = placement
            .source_id
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        Self {
            scene,
            source_id: placement.source_id,
            x: placement.x,
            y: placement.y,
            vy: 0,
            grounded: false,
            initial_direction: placement.initial_direction as i8,
            spawn_quarter: ((placement.rotation_q16 + 45 * ONE).div_euclid(90 * ONE)).rem_euclid(4) as i8,
            spawn_state: SpawnState::Pending,
            health: ActorHealth::new(spec.health),
            walk: WalkState::new(
                placement.initial_direction,
                placement.random_start_direction && seed & 0x80000000 != 0,
            ),
            recoil_left: 0,
            recoil: [0; 2],
            flash_left: 0,
            corpse: None,
            geo_paid: false,
            dream_taken: false,
            located: crate::disc::Located::NONE,
            runtime: match spec.controller {
                ActorController::Crawler => Runtime::Walker,
                ActorController::Runner { params, .. } => {
                    // A mirrored placement starts facing right; the cooker takes
                    // the sign from the transform, so both are supported now.
                    assert!(placement.initial_direction.abs() == 1 && !placement.random_start_direction,
                        "unsupported Runner initial facing variant");
                    Runtime::Runner(RunnerRuntime {
                        controller: hk_sim::runner::Runner::with_facing(params, placement.initial_direction as i8), rng: seed,
                        vx: 0, animation_tick: 0 })
                }
                ActorController::Climber { .. } => Runtime::Climber(ClimberRuntime {
                    // Unit positive scale is the recognizer's contract, so the
                    // authored rotation is the whole of the starting pose.
                    controller: hk_sim::climber::Climber::new([placement.x, placement.y], placement.rotation_q16, 1, placement.start_right)
                        .expect("validated Climber placement"),
                    velocity: [0; 2], animation_tick: 0, clip: hk_sim::climber::Clip::Walk, end_token: None }),
                ActorController::Vengefly { .. } => Runtime::Vengefly(VengeflyRuntime {
                    controller: hk_sim::vengefly::Vengefly::new([placement.x, placement.y], seed), animation_tick: 0, clip: hk_sim::vengefly::Clip::Idle }),
                ActorController::Gruzzer => Runtime::Gruzzer(GruzzerRuntime { controller: hk_sim::gruzzer::Gruzzer::new(seed),
                    animation_tick: 0, reserve: false, parked: false, reported: false }),
                ActorController::GruzzerReserve { .. } => Runtime::Gruzzer(GruzzerRuntime { controller: hk_sim::gruzzer::Gruzzer::new(seed),
                    animation_tick: 0, reserve: true, parked: true, reported: false }),
                // `Battle Control`'s `Init`: a won arena takes `Activate`, which
                // destroys the gates and the `Giant Fly` (`sync_region` then
                // seats it dead and the reserve stays parked for good).
                ActorController::GruzMother => {
                    let activated = crate::persist::get(crate::persist::Kind::BattleScene, scene, 0).is_some();
                    let arena = hk_sim::boss::Arena::new(activated);
                    crate::battle_gates::arena_entry(scene,
                        arena.enter().contains(hk_sim::boss::Action::QuickOpenGates));
                    let controller = if activated { hk_sim::gruz_mother::GruzMother::gone() }
                        else { hk_sim::gruz_mother::GruzMother::new(seed) };
                    Runtime::GruzMother(GruzRuntime { controller, arena, tick: 0, separated: false, rumble: false, grounded: false,
                        box_sprite: NO_BOX })
                }
                // Facing left as authored (the art faces left); `Idle`'s first
                // FaceObject turns it toward the hero.
                ActorController::AcidFlyer { amount, speed, lead, .. } => Runtime::AcidFlyer(AcidFlyerRuntime {
                    controller: hk_sim::acid_flyer::AcidFlyer::with_lead(amount, speed, lead), origin_y: placement.y,
                    animation_tick: 0 }),
                ActorController::MossWalker { .. } => Runtime::MossWalker(MossWalkerRuntime {
                    controller: hk_sim::moss_walker::MossWalker::new(-1, placement.start_alert, seed), animation_tick: 0 }),
                ActorController::Mosquito { .. } => Runtime::Mosquito(MosquitoRuntime {
                    controller: hk_sim::mosquito::Mosquito::new([placement.x, placement.y], seed), animation_tick: 0,
                    hit_terrain: false }),
                ActorController::Baldur { .. } => Runtime::Baldur(BaldurRuntime { controller: hk_sim::baldur::Baldur::new(seed), animation_tick: 0,
                    clip: hk_sim::baldur::Clip::Idle, vx: 0, wall: false }),
                ActorController::Aspid { .. } => {
                    let controller = if placement.start_alert { hk_sim::aspid::Aspid::new_alert([placement.x, placement.y], seed) }
                        else { hk_sim::aspid::Aspid::new([placement.x, placement.y], seed) };
                    Runtime::Aspid(AspidRuntime { controller, animation_tick: 10, clip: hk_sim::aspid::Clip::Fly })
                }
                ActorController::Hatcher { .. } => {
                    let controller = if placement.start_alert { hk_sim::hatcher::Hatcher::new_alert([placement.x, placement.y], seed) }
                        else { hk_sim::hatcher::Hatcher::new([placement.x, placement.y], seed) };
                    Runtime::Hatcher(HatcherRuntime { controller, animation_tick: hk_sim::hatcher::FLY_START_TICKS,
                        clip: hk_sim::hatcher::Clip::Fly })
                }
                // `Init` parks it where the cooker found it, which is the cage.
                ActorController::HatcherBaby => Runtime::HatcherBaby(BabyRuntime {
                    controller: hk_sim::hatcher::Baby::new(seed), animation_tick: 0 }),
                ActorController::ZombieShield { .. } => {
                    // Its Walker carries the Runner's own `rightScale` of -1,
                    // so the cooked sign is the placement's transform mirror.
                    assert!(placement.initial_direction.abs() == 1 && !placement.random_start_direction,
                        "unsupported Zombie Shield initial facing variant");
                    Runtime::ZombieShield(ZombieShieldRuntime {
                        controller: hk_sim::zombie_shield::ZombieShield::with_facing(placement.initial_direction as i8),
                        rng: seed, animation_tick: 0 })
                }
                ActorController::HuskGuard { .. } => Runtime::HuskGuard(HuskGuardRuntime {
                    controller: hk_sim::husk_guard::HuskGuard::new(placement.x, placement.initial_direction as i8, seed),
                    animation_tick: 0, swipe_left: 0 }),
                // `Init` plays `Closed` and raises the shell; `Direction` then
                // branches on the FSM's own `Facing Right`, not on the
                // transform, so nothing of the placement's mirror reaches the
                // controller. The placement's `initial_direction` still carries
                // it, for the sprite.
                ActorController::Blocker { sleeps, .. } => Runtime::Blocker(BlockerRuntime {
                    controller: hk_sim::blocker::Blocker::new(sleeps), rng: seed, animation_tick: 0 }),
                // Which idle loop it plays and which frame of it are draws off
                // this seed, and so is the mirror, which `Set Frame` flips from
                // the placement's authored one rather than writing outright.
                ActorController::Pigeon { .. } => Runtime::Pigeon(PigeonRuntime {
                    controller: hk_sim::pigeon::Pigeon::new(seed, placement.initial_direction as i8),
                    animation_tick: 0 }),
                ActorController::Static { .. } => Runtime::Static { animation_tick: 0 },
                // `initial_direction` carries the sign the cooker read off the
                // placement's transform scale, and a positive source scale is
                // the art as authored, which that field writes as -1.
                //
                // `Pause` -> `Init`, which the source takes on the arena's first
                // frame. The `Battle Scene`'s PersistentBoolItem is a SceneData
                // item and `falseKnightFirstPlop` a PlayerData bool, both in
                // `persist`, so a cleared arena is still cleared after a scene
                // reload, a death respawn or a boot from the card.
                // `Pause` -> `Init` -> `Dormant`, or `Battle Control`'s
                // `Activate` destroying the body when the arena is already won
                // (`sync_region` then seats it dead). `initial_direction` is
                // the Walker's facing, from the placement's scale against its
                // negative rightScale.
                ActorController::Mawlek { .. } => {
                    // A scene load recreates the hero's `Roar Lock` and the
                    // arena at their start states.
                    unsafe { ROAR_LOCK = false; MW_PIECE_WAIT = false; }
                    let activated = crate::persist::get(crate::persist::Kind::BattleScene, scene, 0).is_some();
                    let arena = hk_sim::boss::Arena::with_end_wait(activated, hk_sim::mawlek::ARENA_END_TICKS);
                    crate::battle_gates::arena_entry(scene,
                        arena.enter().contains(hk_sim::boss::Action::QuickOpenGates));
                    let controller = if activated { hk_sim::mawlek::Mawlek::gone() } else {
                        hk_sim::mawlek::Mawlek::new(seed, placement.x, placement.initial_direction as i8)
                    };
                    let mut boss = MawlekRuntime { controller, arena, clips: [0; 5], ticks: [0; 5], spit: 0,
                        vx: 0, gravity: 0, mesh: true, vulnerable: false, arm_hitbox: [false; 2],
                        parried: [false; 2], separated: false, blown: activated };
                    for (part, clip) in hk_sim::mawlek::Mawlek::INITIAL_CLIPS { boss.play(part, clip); }
                    Runtime::Mawlek(boss)
                }
                ActorController::FalseKnight { .. } => {
                    let arena = hk_sim::boss::Arena::new(
                        crate::persist::get(crate::persist::Kind::BattleScene, scene, 0).is_some());
                    let first_plop = crate::persist::player(crate::persist::FALSE_KNIGHT_FIRST_PLOP);
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
                        summon: 0, separated: false,
                        head_clip: hk_sim::false_knight::HeadClip::Idle, head_tick: 0, blown: false,
                        death_head: 0, death_head_tick: 0, death_head_travel: 0 })
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
            let q = climber.quarter() - self.spawn_quarter as i32;
            let corners = [rotate_quarter([b[0], b[1]], q), rotate_quarter([b[2], b[3]], q)];
            return [corners[0][0].min(corners[1][0]), corners[0][1].min(corners[1][1]),
                corners[0][0].max(corners[1][0]), corners[0][1].max(corners[1][1])];
        }
        if self.is_flyer() || self.walk.direction == self.initial_direction as i32 {
            b
        } else {
            [-b[2], b[1], -b[0], b[3]]
        }
    }
    /// One 60 Hz step of the kinematic Climber: motion along its surface
    /// direction, then the controller's ray-driven sensing or turn/stun timers.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
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
        // Every source Climber keeps its authored +1 scale and never flips it,
        // so the art is drawn as authored: -1 in this file's mirror convention,
        // where +1 mirrors (see `prepare_draws`). A vertical-strike corpse
        // takes this sign too.
        self.walk.direction = -1;
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
                let (edges,slot)=actor_edges(state,active,active_room,target,target_room);
                unsafe {ACTOR_SLOT=slot;}
                self.advance_body(spec,active_room,count,|i|edges[i],context,emit);
                unsafe {ACTOR_SLOT=EDGE_SLOTS;}
            }
        } else {
            self.advance_body(spec,active_room,count,|i|state.edge_in_view(active,active_room,target,target_room,i),context,emit);
        }
    }
    fn advance_body(&mut self,spec:&ActorSpec,room:&Room,count:usize,edge:impl Fn(usize)->[i32;4],context:RunnerContext,emit:&mut impl FnMut(RunnerEvent)) {
        if self.health.dead {
            if let (Some(corpse),Some(corpse_spec))=(&mut self.corpse,spec.corpse) {
                corpse.tick(corpse_spec,count,&edge);
            }
            // `End Wait` runs after the boss's death event, so the arena has to
            // keep counting for the two seconds the source waits before BG OPEN.
            if let Some(mut boss)=self.false_knight() {
                let actions=boss.arena.tick();
                self.apply_arena(&mut boss,actions);
                self.seat_false_knight(boss);
            }
            // The corpse runs its own FSM and the arena its 10.5 s after it.
            if let Some(mut boss)=self.mawlek() {
                self.advance_mawlek_corpse(&mut boss,count,&edge,emit);
                self.seat_mawlek(boss);
            }
            // Gruz Mother's corpse, then its burster; the arena counts the
            // reserve flies down behind them.
            if let Some(mut boss)=self.gruz() {
                self.advance_gruz_dead(&mut boss,count,&edge,emit);
                self.seat_gruz(boss);
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
        if let Some(mut fly) = self.acid_flyer() {
            self.spawn_state = SpawnState::Ready;
            self.advance_acid_flyer(&mut fly, context);
            self.runtime = Runtime::AcidFlyer(fly);
            return;
        }
        if let Some(mut m) = self.moss_walker() {
            self.advance_moss_walker(&mut m, spec, count, &edge, context);
            self.runtime = Runtime::MossWalker(m);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut m) = self.mosquito() {
            self.spawn_state = SpawnState::Ready;
            self.advance_mosquito(&mut m, spec, count, &edge, context);
            self.runtime = Runtime::Mosquito(m);
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
        if let Some(mut hatcher) = self.hatcher() {
            self.spawn_state = SpawnState::Ready;
            self.advance_hatcher(&mut hatcher, spec, count, &edge, context, emit);
            self.runtime = Runtime::Hatcher(hatcher);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut baby) = self.baby() {
            self.spawn_state = SpawnState::Ready;
            self.advance_baby(&mut baby, spec, count, &edge, context);
            self.runtime = Runtime::HatcherBaby(baby);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut boss) = self.gruz() {
            self.advance_gruz(&mut boss, spec, count, &edge, context, emit);
            self.seat_gruz(boss);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut boss) = self.mawlek() {
            self.advance_mawlek(&mut boss, spec, count, &edge, context, emit);
            self.seat_mawlek(boss);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut boss) = self.false_knight() {
            self.advance_false_knight(&mut boss, spec, count, &edge, context, emit);
            self.seat_false_knight(boss);
            assert!(self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE, "enemy left validated Q16 scene bounds");
            return;
        }
        if let Some(mut blocker) = self.blocker() {
            // Also bodiless: the Blocker has no Rigidbody2D and no Recoil, so
            // it keeps its authored transform for its whole life and never
            // reaches the terrain solver below.
            self.spawn_state = SpawnState::Ready;
            self.advance_blocker(&mut blocker, spec, room, context, emit);
            self.runtime = Runtime::Blocker(blocker);
            return;
        }
        if let Some(mut bird) = self.pigeon() {
            // Bodiless for a different reason from the Blocker's: the Pigeon
            // has a Rigidbody2D, but its only collider is a trigger, so the
            // source body never touches terrain either and there is nothing
            // here for the solver to do.
            self.spawn_state = SpawnState::Ready;
            self.advance_pigeon(&mut bird, count, &edge, context);
            self.runtime = Runtime::Pigeon(bird);
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
        //
        // The Zombie Shield carries the same `Walker` and is read by the same
        // Sweep model, so it takes the same clearance. That is the one number
        // here borrowed rather than measured on this placement.
        let foot_skin=if self.runner().is_some()||self.zombie_shield().is_some() {983}else{0};
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
        } else if let Some(mut shield) = self.zombie_shield() {
            self.advance_zombie_shield(&mut shield, spec, room, count, &edge, context, emit)
        } else if let Some(mut guard) = self.husk_guard() {
            self.advance_husk_guard(&mut guard, spec, room, count, &edge, context, emit)
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
    // The Husk Warrior's and the Husk Guard's ticks, each its own function so
    // the husks can stream as a code module. Return the walking speed.
    #[inline(never)]
    fn advance_zombie_shield(&mut self, shield: &mut ZombieShieldRuntime, spec: &ActorSpec, room: &Room, count: usize,
                             edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) -> i32 {
        shield.animation_tick = shield.animation_tick.saturating_add(1);
        let senses = self.shield_senses(shield, spec, room, context, count, edge);
        let actions = shield.controller.tick(senses, |e| ZombieShieldRuntime::counter(&mut shield.rng, e));
        for action in actions.iter() {
            // Velocity and facing are read back off the controller. The
            // Walker's loop is the only sound this FSM asks for, and the
            // Shield's AudioSource is the Runner's: `zombie_five_footstep`,
            // Loop on, pitch 0.9974 and the same rolloff (level54), so it
            // plays through the resident Runner bank's loop voices.
            use hk_sim::zombie_shield::Action;
            match action {
                Action::Play(_) => shield.animation_tick = 0,
                Action::AudioPlay => self.emit_runner(RunnerEventKind::AudioPlay, emit),
                Action::AudioStop => self.emit_runner(RunnerEventKind::AudioStop, emit),
                Action::Velocity(_) => {}
            }
        }
        self.walk.direction = shield.controller.facing();
        let vx = shield.controller.velocity_x();
        self.runtime = Runtime::ZombieShield(*shield);
        vx
    }
    #[inline(never)]
    fn advance_husk_guard(&mut self, guard: &mut HuskGuardRuntime, spec: &ActorSpec, room: &Room, count: usize,
                          edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) -> i32 {
        use hk_sim::husk_guard::{self as g, Action};
        guard.animation_tick = guard.animation_tick.saturating_add(1);
        guard.swipe_left = guard.swipe_left.saturating_sub(1);
        let pos = [self.x, self.y];
        let facing = guard.controller.facing();
        let world = |b| g::facing_box(b, facing, pos);
        let in_alert_range = overlap(world(g::ALERT), context.hero_body);
        let can_see_hero = in_alert_range && hk_sim::runner_senses::line_of_sight(pos, context.hero, true, count, edge)
            .unwrap_or(false);
        let senses = g::Senses {
            position: pos, hero: context.hero, can_see_hero, in_alert_range,
            in_attack_range: overlap(world(g::ATTACK), context.hero_body),
            hero_above: overlap(world(g::OVERHEAD), context.hero_body),
            completed: guard.completed(spec, room),
        };
        let actions = guard.controller.tick(senses);
        let ActorController::HuskGuard { spurt_clip, slam_clip, .. } = spec.controller
            else { panic!("Husk Guard runtime with other metadata") };
        for action in actions.iter() {
            match action {
                Action::Play(_) => guard.animation_tick = 0,
                Action::Swipe => guard.swipe_left = g::SWIPE_TICKS,
                Action::Slam(p) => emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: p,
                    facing, kind: RunnerEventKind::Effect { clip: slam_clip } }),
                Action::Shockwaves(p) => for dir in [1, -1] {
                    emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: p,
                        facing: dir, kind: RunnerEventKind::GuardWave { spurt_clip } });
                },
            }
        }
        self.walk.direction = facing;
        let vx = guard.controller.velocity_x();
        self.walk.direction = guard.controller.facing();
        self.runtime = Runtime::HuskGuard(*guard);
        vx
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
    // Its own function so the family can stream as a code module.
    #[inline(never)]
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
    /// One 60 Hz step of the Moss Walker: the senses its FSM reads this tick
    /// (the wake circle and line while buried or due a hide check, the
    /// child point rays every third Walking frame, Ground Range for a turn),
    /// the FSM, then the gravity body at the velocity it last set.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_moss_walker(&mut self, m: &mut MossWalkerRuntime, spec: &ActorSpec, count: usize,
                           edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        use hk_sim::moss_walker as mw;
        use hk_sim::runner_senses as q;
        let need = m.controller.needs();
        let mut senses = mw::Senses::default();
        let pos = [self.x, self.y];
        if need.wake_range {
            let hb = context.hero_body;
            let dx = (pos[0].clamp(hb[0], hb[2]) - pos[0]) as i64;
            let dy = (pos[1].clamp(hb[1], hb[3]) - pos[1]) as i64;
            let r = mw::WAKE_RADIUS as i64;
            let in_circle = dx * dx + dy * dy <= r * r;
            senses.wake_range = match q::line_of_sight(pos, context.hero, in_circle, count, edge) {
                Ok(value) => value,
                Err(q::QueryError::ZeroLengthSight) => in_circle,
                Err(_) => panic!("Moss Walker sight outside validated coordinates"),
            };
        }
        // The art faces left at scale +1, so the scale sign is minus the facing.
        let ray = |origin: [i32; 2], direction: [i32; 2], length: i32| climber_ray_hit(hk_sim::climber::Ray {
            position: pos, rotation_degrees_q16: 0, scale_x_sign: -m.controller.facing() as i8,
            local_origin: origin, local_direction: direction, length, layer_mask: 256 }, count, edge).is_some();
        if need.walk_rays {
            senses.edge = ray(mw::EDGE_ORIGIN, [0, -ONE], mw::EDGE_LENGTH);
            senses.wall = ray(mw::WALL_ORIGIN, [-ONE, 0], mw::WALL_LENGTH);
        }
        if need.ground_ray {
            senses.ground = ray(mw::GROUND_ORIGIN, [0, -ONE], mw::GROUND_LENGTH);
        }
        let step = m.controller.tick(senses);
        m.animation_tick = if step.play.is_some() { 0 } else { m.animation_tick.saturating_add(1) };
        self.walk.direction = m.controller.facing();
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let vx = m.controller.vx();
        let p = Params { speed: vx.abs(), gravity: 60 * ONE, fall: 100 * ONE, half_width: (b[2] - b[0]) / 2,
            bottom: b[1], top: b[3], ..Params::ZERO };
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        if self.spawn_state != SpawnState::Ready {
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            self.spawn_state = SpawnState::Ready;
        }
        let before = body.x;
        body.step(p, vx.signum(), false, count, edge);
        // A frictionless body against a wall: Box2D zeroes the normal velocity.
        if vx != 0 && body.x - before != vx / 60 { m.controller.stopped(); }
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            let mut p = p;
            p.speed = self.recoil[0].abs();
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        self.vy = body.vy;
        self.grounded = body.grounded;
    }
    /// One 60 Hz step of the Mosquito: senses (the 8.68 alert circle against
    /// the hero body, then the terrain ray), the FSM, and the gravity-free
    /// body against terrain with Recoil. The box it solves with is the body
    /// united with `TileDetector` while that lives.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_mosquito(&mut self, m: &mut MosquitoRuntime, spec: &ActorSpec, count: usize, edge: &impl Fn(usize) -> [i32; 4],
                        context: RunnerContext) {
        use hk_sim::runner_senses as q;
        let pos = [self.x, self.y];
        let hb = context.hero_body;
        let dx = (pos[0].clamp(hb[0], hb[2]) - pos[0]) as i64;
        let dy = (pos[1].clamp(hb[1], hb[3]) - pos[1]) as i64;
        let r = hk_sim::mosquito::ALERT_RADIUS as i64;
        let in_range = dx * dx + dy * dy <= r * r;
        let can_see_hero = match q::line_of_sight(pos, context.hero, in_range, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Mosquito LOS outside validated coordinates"),
        };
        let senses = hk_sim::mosquito::Senses { position: pos, hero: context.hero, can_see_hero, hit_terrain: m.hit_terrain };
        let step = m.controller.tick(senses);
        m.animation_tick = if step.play.is_some() { 0 } else { m.animation_tick.saturating_add(1) };
        self.walk.direction = m.controller.facing();
        let mut b = self.local_bounds(spec);
        if let Some(t) = self.mosquito_tile(spec) {
            b = [b[0].min(t[0] - self.x), b[1].min(t[1] - self.y), b[2].max(t[2] - self.x), b[3].max(t[3] - self.y)];
        }
        let offset = b[0] + (b[2] - b[0]) / 2;
        let v = m.controller.velocity();
        let mut body = Player::spawn(self.x + offset, self.y);
        let mut p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        body.vy = v[1];
        body.step(p, v[0].signum(), false, count, edge);
        let moved = [body.x - (self.x + offset), body.y - self.y];
        m.hit_terrain = (v[0] != 0 && moved[0] != v[0] / 60) || (v[1] != 0 && moved[1] != v[1] / 60);
        if self.recoil_left != 0 {
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
    }
    /// `Tween` moves the transform (the dynamic body never gets a velocity)
    /// and `Idle`'s FaceObject turns it to the hero, restarting TurnToFly.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_acid_flyer(&mut self, fly: &mut AcidFlyerRuntime, context: RunnerContext) {
        if fly.controller.tick(self.x, context.hero[0]).is_some() {
            fly.animation_tick = 0;
        } else {
            fly.animation_tick = fly.animation_tick.saturating_add(1);
        }
        self.walk.direction = if fly.controller.facing_right() { 1 } else { -1 };
        self.y = fly.origin_y + fly.controller.offset();
    }
    /// One 60 Hz step of the Gruzzer: camera wake, constant-speed flight along
    /// the controller angle against terrain, Recoil displacement, then the
    /// blocked side (up, right, down, left, as CheckCollisionSide) re-aims.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
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
    /// The Pigeon's senses: the `Hero Range` child's circle against the hero
    /// body box, and the raycast its FSM only runs while something is inside
    /// that circle. `line_of_sight` short-circuits on `in_range`, so a perched
    /// bird pays one squared distance a tick and visits no terrain at all.
    ///
    /// `Enemy Range`, the second trigger child, is not read. Its own raycast
    /// writes the same hero visibility into the same FSM, and its circle is
    /// contained in this one (centres 0.0544 apart, radii 2.824 and 5.072), so
    /// geometrically it can add nothing. What it would add is *who* may trip
    /// it, which is the flock cascade named in the recognizer's limitations.
    fn pigeon_senses(&self, context: RunnerContext, count: usize, edge: &impl Fn(usize) -> [i32; 4]) -> hk_sim::pigeon::Senses {
        use hk_sim::pigeon as p;
        use hk_sim::runner_senses as q;
        let eye = [self.x + p::HERO_RANGE_CENTER[0], self.y + p::HERO_RANGE_CENTER[1]];
        let hb = context.hero_body;
        let dx = (eye[0].clamp(hb[0], hb[2]) - eye[0]) as i64;
        let dy = (eye[1].clamp(hb[1], hb[3]) - eye[1]) as i64;
        let radius = p::HERO_RANGE_RADIUS as i64;
        let in_range = dx * dx + dy * dy <= radius * radius;
        let can_see_hero = match q::line_of_sight(eye, context.hero, in_range, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Pigeon LOS outside validated coordinates"),
        };
        p::Senses { can_see_hero, hero_right: context.hero[0] > self.x }
    }
    /// One 60 Hz step of the Pigeon: the circle and its gated raycast, the FSM
    /// tick, then a trigger body that moves by its own velocity and by nothing
    /// else. No terrain solve, no gravity, no Recoil: the source object has
    /// none of the three.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_pigeon(&mut self, bird: &mut PigeonRuntime, count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        use hk_sim::pigeon::{Action, Phase, WORLD_EDGE};
        if matches!(bird.controller.phase(), Phase::Gone | Phase::Dead) {
            return;
        }
        bird.animation_tick = bird.animation_tick.saturating_add(1);
        let senses = self.pigeon_senses(context, count, edge);
        for action in bird.controller.tick(senses).iter() {
            match action {
                Action::Play(_, tick) => bird.animation_tick = tick,
                Action::Rise(dy) => self.y += dy,
            }
        }
        // `Invert Scale` and the flight direction are the same mirror the
        // cooked box is measured against, so the hurt box follows the sprite.
        self.walk.direction = bird.controller.facing();
        let v = bird.controller.velocity();
        self.x += v[0] / 60;
        self.y += v[1] / 60;
        // The source keeps the object for five seconds, by which point it is
        // hundreds of units outside the room. Take it off the board at the edge
        // of the validated coordinate range rather than let it leave it.
        if self.x.abs() > WORLD_EDGE || self.y.abs() > WORLD_EDGE {
            self.x = self.x.clamp(-WORLD_EDGE, WORLD_EDGE);
            self.y = self.y.clamp(-WORLD_EDGE, WORLD_EDGE);
            bird.controller.leave();
        }
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
                    crate::persist::set(crate::persist::Kind::BattleScene, self.scene, 0, 1)
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
                    // FalseyControl's Music state follows the entrance drop:
                    // Boss1 as an XA song for the fight.
                    crate::music::boss(true);
                    // The same state raises the area title with FALSE_KNIGHT.
                    crate::title_card::show_boss(crate::title_card::FALSE_KNIGHT);
                    let cleared = boss.arena.kill_all_enemies();
                    self.apply_arena(boss, cleared);
                }
                Action::Died => {
                    crate::music::boss(false);
                    boss.arena.enemy_died();
                    self.health.dead = true;
                    // The death state's own PlayerData write, apart from the
                    // arena's: the source keeps both.
                    crate::persist::set_player(crate::persist::FALSE_KNIGHT_DEFEATED);
                    unsafe {
                        HK_FK_DEATHS = HK_FK_DEATHS.saturating_add(1);
                        HK_FK_KILL_CHEATS = crate::cheats::HK_CHEATS;
                    }
                }
                // `Pause Long` sets `falseKnightFirstPlop`, so every later
                // stagger takes the short pause, in this fight and in every
                // one after it that the card carries.
                Action::SetFirstPlop => {
                    crate::persist::set_player(crate::persist::FALSE_KNIGHT_FIRST_PLOP)
                }
                Action::Staggered(_) => unsafe {
                    HK_FK_CONVERSIONS = HK_FK_CONVERSIONS.saturating_add(1)
                },
                Action::Head(clip) => {
                    if clip == hk_sim::false_knight::HeadClip::Hit {
                        unsafe { HK_FK_HEAD_HITS = HK_FK_HEAD_HITS.saturating_add(1) }
                    }
                    boss.head_clip = clip;
                    boss.head_tick = 0;
                }
                // `Set Head Facing` then `Blow`: the Death Head leaves the
                // armour on the side the body faces, at 4 units a second.
                Action::Blow => {
                    boss.blown = true;
                    boss.death_head = 1;
                    boss.death_head_tick = 0;
                    boss.death_head_travel = 0;
                }
                Action::DeathHeadLand => {
                    boss.death_head = 3;
                    boss.death_head_tick = 0;
                }
                Action::CrackFloor => crate::battle_gates::floor_crack(),
                Action::BreakFloor => crate::battle_gates::floor_break(),
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
                    // The rest of the fight's voices are Crossroads_10's own
                    // scene bank (host/scene_sfx.py), on the scene voice; the
                    // two long roars take the shared voice, which nothing else
                    // uses while they sound, so a slam does not cut them.
                    Effect::Slam => {
                        crate::camera::request(Shake::Big);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_STRIKE_GROUND);
                    }
                    Effect::StunStart => {
                        crate::camera::request(Shake::Big);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_DAMAGE_ARMOUR_FINAL);
                    }
                    Effect::Steam => {
                        crate::camera::request(Shake::Big);
                        crate::scene_sfx::play_shared(crate::scene_sfx::FALSE_KNIGHT_DEATH);
                    }
                    Effect::Blow => crate::camera::request(Shake::Big),
                    // `Floor Break` also takes the mixer to `Silent` over two
                    // seconds, which is where the fight's music ends: the Head
                    // is finished to the sound of the room, not the theme.
                    Effect::FloorBreak => {
                        crate::camera::request(Shake::Big);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_STRIKE_GROUND);
                        crate::music::boss_silence(crate::fk_art::FK_FLOOR_BREAK_SILENCE_TICKS);
                    }
                    Effect::RageSlam => {
                        crate::camera::request(Shake::Average);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_STRIKE_GROUND);
                    }
                    Effect::JumpShake => {
                        crate::camera::request(Shake::Kill);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_JUMP);
                    }
                    Effect::Jump => crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_JUMP),
                    Effect::RageJump => {
                        crate::camera::request(Shake::Kill);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_JUMP);
                        crate::scene_sfx::play_shared(crate::scene_sfx::FALSE_KNIGHT_RAGE);
                    }
                    Effect::Entrance => {
                        crate::camera::request(Shake::Big);
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_CEILING_BREAK);
                    }
                    Effect::EntranceLanding => {
                        crate::camera::request(Shake::Average);
                        crate::audio::boss_land();
                        crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_LAND_1ST_TIME);
                    }
                    Effect::Roll => crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_ROLL),
                    Effect::RunStart => crate::scene_sfx::play(crate::scene_sfx::ZOMBIE_GUARD_FOOTSTEP),
                    // `Open Uuup` and `Death Open` each fire two
                    // AudioPlayerOneShotSingle actions, the raise and the
                    // move, through separate audio players, so they sound
                    // together: the move takes the shared voice because a
                    // second play on the scene voice would retrigger it.
                    Effect::ArmourOpen => {
                        crate::scene_sfx::play(crate::scene_sfx::ZOMBIE_SHIELD_RAISE);
                        crate::scene_sfx::play_shared(crate::scene_sfx::ZOMBIE_SHIELD_MOVE);
                    }
                    Effect::DeathLand => crate::scene_sfx::play(crate::scene_sfx::FALSE_KNIGHT_LAND_1ST_TIME),
                    // Six head-hit cries and five attack cries, none in any bank.
                    Effect::HeadHit | Effect::Voice => {}
                    // `Death Anim Start` writes `falseKnightDefeated` itself,
                    // ahead of the death event; the arena's own write follows.
                    Effect::DeathStart => {
                        crate::camera::request(Shake::Kill);
                        crate::scene_sfx::play(crate::scene_sfx::BOSS_FINAL_HIT);
                        crate::persist::set_player(crate::persist::FALSE_KNIGHT_DEFEATED);
                    }
                },
                // SUMMON, with `Spawns` written. The summoner is a different
                // object with its own clock, so this is only the send: it is
                // held here and drained once, in `advance_false_knight`, rather
                // than emitted from each of the three places the boss is
                // written back from. Only `tick` ever produces one, and holding
                // it means the other two exits cannot quietly drop it.
                Action::SummonBarrels(spawns) => boss.summon = spawns,
                // Read straight off `tick`'s list in `advance_false_knight`,
                // the only exit that can produce one, so it costs the runtime
                // no byte in each of the thirty-two actor slots.
                Action::Shockwave { .. } => {}
                // `Invincible` and `HeadExposed` are read back off the phase
                // rather than latched, so there is one source of truth for
                // which target the nail reaches.
                //
                Action::Invincible(_) | Action::HeadExposed(_) => {}
            }
        }
    }
    /// `Battle Control` answered for the Mawlek's arena.
    /// `Battle Control`'s actions for the Mawlek's and Gruz Mother's arenas:
    /// gates and the `Activated` write. Resident and shared, because the two
    /// code modules carried identical copies, which the linker's identical-
    /// code folding merged into one module that the other then jumped into.
    fn apply_battle_scene(&mut self, actions: hk_sim::boss::Actions) {
        for action in actions.iter() {
            match action {
                hk_sim::boss::Action::CloseGates => crate::battle_gates::close(self.scene),
                hk_sim::boss::Action::OpenGates
                | hk_sim::boss::Action::QuickOpenGates => crate::battle_gates::open(self.scene),
                // `Blow Wait` (Mawlek) and the corpse's `Init` (Gruz Mother)
                // write `Activated` first thing; the card takes it at the next
                // bench, as the False Knight's does.
                hk_sim::boss::Action::Persist => {
                    crate::persist::set(crate::persist::Kind::BattleScene, self.scene, 0, 1)
                }
                // BATTLE START answers the boss that sent START; the camera
                // lock is the authored `CameraLockArea`s; neither arena has a
                // floor to break, and KILL ALL ENEMIES only reaches Gruz
                // Mother's counting `Start`.
                hk_sim::boss::Action::StartBattle
                | hk_sim::boss::Action::ActivateFloor
                | hk_sim::boss::Action::CameraLock(_)
                | hk_sim::boss::Action::KillMinions => {}
            }
        }
    }
    /// One action list of the Mawlek's five state machines against the guest.
    /// Not generic and never inlined: it runs from the tick, the nail and the
    /// corpse, and one copy of it is 2.7 KB of code the RAM budget pays for.
    #[inline(never)]
    fn apply_mawlek(&mut self, boss: &mut MawlekRuntime, actions: hk_sim::mawlek::Actions,
                    emit: &mut dyn FnMut(RunnerEvent)) {
        use crate::camera::Shake;
        use crate::scene_sfx as sfx;
        use hk_sim::mawlek::{Action, Effect, Part};
        for action in actions.iter() {
            match action {
                Action::Play(part, clip) => {
                    boss.play(part, clip);
                    if let Part::Arm(_) = part {
                        if clip == hk_sim::mawlek::Clip::ArmSwipeAntic { unsafe { HK_MW_SWIPES = HK_MW_SWIPES.saturating_add(1) } }
                    }
                    if part == Part::Dummy && clip == hk_sim::mawlek::Clip::DummyJumpAntic {
                        unsafe { HK_MW_LEAPS = HK_MW_LEAPS.saturating_add(1) }
                    }
                }
                Action::Mesh(on) => boss.mesh = on,
                Action::Velocity([x, y]) => { boss.vx = x; self.vy = y; self.grounded = false; }
                Action::VelocityX(x) => boss.vx = x,
                Action::Gravity(g) => boss.gravity = g,
                Action::Vulnerable => boss.vulnerable = true,
                Action::ArmHitbox(arm, on) => {
                    boss.arm_hitbox[arm as usize] = on;
                    if on { boss.parried[arm as usize] = false; }
                }
                // Read back off the controller where they are used.
                Action::DummyScale(_) | Action::HeadCollider(_) => {}
                Action::Spray { count, speed, angles } => {
                    unsafe { HK_MW_SPRAYS = HK_MW_SPRAYS.saturating_add(1) }
                    let seed = boss.controller.random();
                    emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: [self.x, self.y],
                        facing: 0, kind: RunnerEventKind::MawlekShots { count, speed, angles, seed } });
                }
                Action::HeadShot { speed, angles } => {
                    unsafe { HK_MW_HEAD_SHOTS = HK_MW_HEAD_SHOTS.saturating_add(1) }
                    let seed = boss.controller.random();
                    let [hx, hy] = crate::mawlek_art::MW_HEAD_OFFSET;
                    emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: [self.x + hx, self.y + hy],
                        facing: 0, kind: RunnerEventKind::MawlekShots { count: 1, speed: [speed, speed], angles, seed } });
                }
                Action::SpitEffect => boss.spit = 1,
                Action::StartBattle => {
                    unsafe { HK_MW_WOKEN = HK_MW_WOKEN.saturating_add(1) }
                    let started = boss.arena.hero_entered();
                    self.apply_battle_scene(started);
                    // No `Kill Zombies` here: the counter waits from `Start`.
                    let waiting = boss.arena.kill_all_enemies();
                    self.apply_battle_scene(waiting);
                }
                Action::Title => crate::title_card::show_boss(crate::title_card::MAWLEK),
                Action::RoarLock(on) => unsafe { ROAR_LOCK = on },
                // `Music`: ApplyMusicCue EnemyBattle, as an XA song like the False
                // Knight's Boss1.
                Action::Music => crate::music::boss_track(crate::music::MAWLEK_TRACK),
                // The corpse's `Music`: BATTLE END, and the mixer to `Silent`
                // over 2 s, which ends the battle music.
                Action::Died => {
                    unsafe { MW_PIECE_WAIT = true; }
                    boss.arena.enemy_died();
                    crate::music::boss_silence(hk_sim::mawlek::DEATH_SILENCE_TICKS);
                    boss.arm_hitbox = [false; 2];
                    unsafe {
                        HK_MW_DEATHS = HK_MW_DEATHS.saturating_add(1);
                        HK_MW_KILL_CHEATS = crate::cheats::HK_CHEATS;
                    }
                }
                // `Blow` lands on `Blow Wait`'s 5.5 s: `End Wait` activates the
                // Heart Piece the same tick.
                Action::Blown => {
                    unsafe { MW_PIECE_WAIT = false; }
                    boss.blown = true;
                    unsafe { HK_MW_BLOWN = HK_MW_BLOWN.saturating_add(1) }
                }
                Action::Effect(effect) => match effect {
                    Effect::WakeJump => {
                        crate::camera::request(Shake::Average);
                        sfx::play(sfx::MAWLEK_JUMP_OFFSCREEN);
                    }
                    Effect::WakeLand => {
                        crate::camera::request(Shake::Big);
                        sfx::play(sfx::ZOMBIE_GUARD_CLUB);
                    }
                    Effect::Roar => sfx::play_shared(sfx::MAWLEK_SCREAM),
                    Effect::BigSpit => sfx::play(sfx::MAWLEK_BIG_SPIT),
                    Effect::Jump => {
                        crate::camera::request(Shake::Kill);
                        sfx::play(sfx::MAWLEK_JUMP);
                    }
                    Effect::Land => {
                        crate::camera::request(Shake::Average);
                        sfx::play(sfx::ZOMBIE_GUARD_CLUB);
                    }
                    Effect::ArmCall => sfx::play(sfx::MAWLEK_CALL),
                    Effect::ArmWhip => sfx::play(sfx::MAWLEK_WHIP),
                    // AudioPlayerOneShot between its two clips, weights 1:1.
                    Effect::HeadSpit => sfx::play(if boss.controller.random() & 1 == 0 { sfx::MAWLEK_SPIT } else { sfx::MAWLEK_SPIT_B }),
                    Effect::CorpseInit => {
                        crate::camera::request(Shake::Average);
                        sfx::play(sfx::BOSS_FINAL_HIT);
                    }
                    Effect::CorpseSteam => {
                        crate::camera::request(Shake::Big);
                        sfx::play_shared(sfx::BOSS_GUSHING);
                    }
                    // `Sting`'s `Boss Defeat`, from XA once (host/hk-cook/src/xa_music.rs).
                    Effect::Sting => crate::music::sting(crate::music::BOSS_DEFEAT_TRACK),
                    Effect::Blow => {
                        crate::camera::request(Shake::Big);
                        sfx::play(sfx::BOSS_EXPLODE);
                    }
                },
            }
        }
    }
    /// One 60 Hz step of Brooding Mawlek and the arena it drives.
    ///
    /// `Dormant` is one box against the hero and the Dummy's lurk loop: the
    /// body is not solved at all until it wakes, because `Init` set its gravity
    /// to 0 and nothing moves it.
    #[inline(never)]
    fn advance_mawlek(&mut self, boss: &mut MawlekRuntime, spec: &ActorSpec,
                      count: usize, edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext,
                      emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::mawlek as mw;
        let ActorController::Mawlek { wake } = spec.controller else { panic!("Mawlek runtime with other metadata") };
        for tick in boss.ticks.iter_mut() { *tick = tick.saturating_add(1); }
        if boss.spit != 0 {
            boss.spit = boss.spit.saturating_add(1);
            let (_, frames, fps, ..) = crate::mawlek_art::MW_ART_CLIPS[crate::mawlek_art::SPIT];
            if boss.spit as u32 > frames as u32 * 60 * 65536 / fps { boss.spit = 0; }
        }
        let phase = boss.controller.phase();
        if phase == mw::Phase::Gone { return; }
        let hero_in_wake = phase == mw::Phase::Dormant && overlap(MawlekRuntime::relative(wake, self.x, self.y), context.hero_body);
        if phase == mw::Phase::Dormant && !hero_in_wake { return; }
        let arena = boss.arena.tick();
        self.apply_battle_scene(arena);
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params {
            speed: boss.vx.abs(),
            gravity: (((60 * ONE) as i64 * boss.gravity as i64) >> 16) as i32,
            fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO
        };
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        if !boss.separated {
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            boss.separated = true;
            self.spawn_state = SpawnState::Ready;
        }
        if phase != mw::Phase::Dormant {
            body.step(p, boss.vx.signum(), false, count, edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        self.vy = body.vy;
        self.grounded = body.grounded;
        // The Walker's Sweeps, only on the ticks it walks: a wall within half
        // a unit of the box and the floor under the edge ahead.
        let facing = boss.controller.walker_facing() as i32;
        let (wall_ahead, floor_ahead) = if boss.controller.walking() {
            let half = (b[2] - b[0]) / 2;
            let cx = self.x + offset;
            let reach = cx + facing * (half + ONE / 2);
            let wall = segment_hits_terrain([cx, self.y], [reach, self.y], count, edge)
                || segment_hits_terrain([cx, self.y + b[1] + ONE / 4], [reach, self.y + b[1] + ONE / 4], count, edge);
            let edge_x = cx + facing * (half + ONE);
            let floor = segment_hits_terrain([edge_x, self.y + b[1] + ONE / 8], [edge_x, self.y + b[1] - ONE / 4], count, edge);
            (wall, floor)
        } else { (false, true) };
        let range = |arm: usize| overlap(MawlekRuntime::relative(crate::mawlek_art::MW_ARM_RANGE[arm], self.x, self.y), context.hero_body);
        let senses = mw::Senses {
            self_x: self.x, hero_x: context.hero[0], head_x: self.x + crate::mawlek_art::MW_HEAD_OFFSET[0],
            grounded: body.grounded, hero_in_wake,
            hero_in_arm: [boss.controller.arm_watching(0) && range(0), boss.controller.arm_watching(1) && range(1)],
            wall_ahead, floor_ahead,
        };
        let actions = boss.controller.tick(senses);
        self.apply_mawlek(boss, actions, &mut |event| emit(event));
    }
    /// The corpse's life: its FSM on the controller, the arena counting down
    /// behind it, and the prefab's own gravity-1 body falling to the floor.
    fn advance_mawlek_corpse(&mut self, boss: &mut MawlekRuntime, count: usize,
                             edge: &impl Fn(usize) -> [i32; 4], emit: &mut impl FnMut(RunnerEvent)) {
        let arena = boss.arena.tick();
        self.apply_battle_scene(arena);
        if boss.blown { return; }
        boss.ticks[1] = boss.ticks[1].saturating_add(1);
        let actions = boss.controller.tick(hk_sim::mawlek::Senses::default());
        self.apply_mawlek(boss, actions, &mut |event| emit(event));
        let b = crate::mawlek_art::MW_CORPSE_BOX;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params { speed: boss.vx.abs(), gravity: 60 * ONE, fall: 100 * ONE, half_width: (b[2] - b[0]) / 2,
            bottom: b[1], top: b[3], ..Params::ZERO };
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        if !boss.separated {
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) { return; }
            boss.separated = true;
        }
        body.step(p, boss.vx.signum(), false, count, edge);
        // ponytail: the corpse stops sliding when it lands rather than running
        // Box2D friction down; add the friction if a frame shows the slide.
        if body.grounded { boss.vx = 0; }
        self.x = body.x - offset;
        self.y = body.y;
        self.vy = body.vy;
        self.grounded = body.grounded;
    }
    /// The nail or a spell against the body's box and, while `Start` has it
    /// on, the Head's: both are one HealthManager, the Head's collider finds
    /// its parent's. Returns whether either box was reached, and the hit.
    #[inline(never)]
    fn strike_mawlek(&mut self, boss: &mut MawlekRuntime, spec: &ActorSpec, hits: &dyn Fn([i32; 4]) -> bool,
                     damage: u16, fling: [i32; 2]) -> (bool, Hit) {
        let head = boss.controller.head_collider()
            && hits(MawlekRuntime::relative(crate::mawlek_art::MW_HEAD_BOX, self.x, self.y));
        if !head && !hits(self.bounds(spec)) {
            return (false, Hit::Ignored);
        }
        let params = hk_sim::EnemyParams { invincible: !boss.vulnerable, ..spec.health };
        let hit = self.health.hit(params, damage);
        if matches!(hit, Hit::Damaged | Hit::Killed) {
            self.flash_left = FLASH_TICKS;
            unsafe { HK_MW_HITS = HK_MW_HITS.saturating_add(1) }
        }
        if hit == Hit::Killed {
            let actions = boss.controller.die();
            self.apply_mawlek(boss, actions, &mut no_runner_event);
            // EnemyDeathEffects spawns the corpse where the body was and flings
            // it at `corpseFlingSpeed` the way the killing blow went.
            boss.separated = false;
            boss.ticks[1] = 0;
            boss.vx = fling[0];
            self.vy = fling[1];
            self.grounded = false;
        }
        (true, hit)
    }
    /// `Battle Control` answered for Gruz Mother's arena.
    /// One action list of Gruz Mother's state machines against the guest.
    #[inline(never)]
    fn apply_gruz(&mut self, boss: &mut GruzRuntime, actions: hk_sim::gruz_mother::Actions,
                  emit: &mut dyn FnMut(RunnerEvent)) {
        use crate::camera::Shake;
        use crate::scene_sfx as sfx;
        use hk_sim::gruz_mother::{self as gz, Action, Sound};
        for action in actions.iter() {
            match action {
                Action::Play(clip) => {
                    boss.tick = 0;
                    match clip {
                        gz::Clip::ChargeAntic if boss.controller.phase() == gz::Phase::ChargeAntic =>
                            unsafe { HK_GZ_CHARGES = HK_GZ_CHARGES.saturating_add(1) },
                        gz::Clip::ChargeAntic => unsafe { HK_GZ_SLAMS = HK_GZ_SLAMS.saturating_add(1) },
                        _ => {}
                    }
                }
                // Read back off the controller where they are used: the
                // invincibility, the `Hero Damager`, the body's loops (the one
                // scene voice holds no AudioSource loop) and the snore.
                Action::Invincible(_) | Action::HeroDamager | Action::Loop(_) | Action::SnoreOff => {}
                Action::Title => crate::title_card::show_boss(crate::title_card::BIGFLY),
                Action::StartBattle => {
                    unsafe { HK_GZ_WOKEN = HK_GZ_WOKEN.saturating_add(1) }
                    let started = boss.arena.hero_entered();
                    self.apply_battle_scene(started);
                    // `Start`: Battle Enemies 7 and an every-frame compare,
                    // which `Clearing` is.
                    boss.arena.set_enemies(gz::BATTLE_ENEMIES);
                    let counting = boss.arena.kill_all_enemies();
                    self.apply_battle_scene(counting);
                }
                // `Fly`'s ApplyMusicCue EnemyBattle, the Mawlek's cue too.
                Action::Music => crate::music::boss_track(crate::music::MAWLEK_TRACK),
                Action::Sound(sound) => sfx::play(match sound {
                    Sound::Startle => sfx::BIG_FLY_SNORE_STARTLE,
                    Sound::WallHit => sfx::BIG_FLY_WALL_HIT,
                    Sound::FinalHit => sfx::GRUZ_FINAL_HIT,
                    Sound::Gushing => sfx::GRUZ_GUSHING,
                    Sound::Explode => sfx::GRUZ_EXPLODE,
                    Sound::Gurgle1 => sfx::BIG_FLY_STOMACHE_PROBLEMS_1,
                    Sound::Gurgle2 => sfx::BIG_FLY_STOMACHE_PROBLEMS_2,
                    Sound::GurgleFinal => sfx::BIG_FLY_STOMACHE_PROBLEMS_FINAL_AND_EXPLODE,
                }),
                Action::Shake(gz::Shake::Average) => crate::camera::request(Shake::Average),
                Action::Shake(gz::Shake::Big) => crate::camera::request(Shake::Big),
                Action::Rumble(on) => boss.rumble = on,
                Action::Impact => unsafe { HK_GZ_IMPACTS = HK_GZ_IMPACTS.saturating_add(1) },
                // The corpse's `Music` (the mixer to `Silent` over 2 s) and its
                // `Init`, which writes the arena's `Activated` at once.
                Action::Died => {
                    crate::music::boss_silence(120);
                    crate::persist::set(crate::persist::Kind::BattleScene, self.scene, 0, 1);
                    unsafe {
                        HK_GZ_DEATHS = HK_GZ_DEATHS.saturating_add(1);
                        HK_GZ_KILL_CHEATS = crate::cheats::HK_CHEATS;
                    }
                }
                Action::Sting => crate::music::sting(crate::music::BOSS_DEFEAT_TRACK),
                Action::Blown => {
                    boss.separated = false;
                    boss.box_sprite = NO_BOX;
                    boss.grounded = false;
                    self.vy = 0;
                    unsafe { HK_GZ_BLOWN = HK_GZ_BLOWN.saturating_add(1) }
                }
                Action::Geo => unsafe { GZ_GEO = Some((self.scene, [self.x, self.y])) },
                Action::Burst => crate::camera::request(Shake::Average),
                Action::ReleaseFlies => {
                    unsafe { HK_GZ_RELEASED = HK_GZ_RELEASED.saturating_add(1) }
                    emit(RunnerEvent { scene: self.scene, source_id: self.source_id, position: [self.x, self.y],
                        facing: boss.facing(), kind: RunnerEventKind::ReleaseReserve });
                }
            }
        }
    }
    /// One 60 Hz step of Gruz Mother alive. Asleep it is one polygon test and
    /// the clip clock: nothing moves it (gravity 0, velocity 0).
    #[inline(never)]
    fn advance_gruz(&mut self, boss: &mut GruzRuntime, spec: &ActorSpec, count: usize,
                    edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::gruz_mother::{self as gz, Motion};
        use hk_sim::gruzzer::Side;
        boss.tick = boss.tick.saturating_add(1);
        if boss.controller.phase() == gz::Phase::Gone { return; }
        let hero = context.hero;
        if boss.controller.asleep() {
            let mut range = crate::gruz_art::GZ_RANGE;
            for p in range.iter_mut() { *p = [self.x + p[0], self.y + p[1]]; }
            let hero_in_range = hk_sim::polygon_hits_box(&range, context.hero_body);
            let actions = boss.controller.tick(gz::Senses { position: [self.x, self.y], hero, hero_in_range, bonk: None, landed: false });
            self.apply_gruz(boss, actions, emit);
            return;
        }
        let arena = boss.arena.tick();
        self.apply_battle_scene(arena);
        let (frame, changed) = boss.frame_box(spec.bounds);
        let b = boss.body(frame);
        let v = match boss.controller.motion() {
            Motion::Vector(v) => v,
            Motion::Polar { angle, speed } => crate::world::debris::rotate([speed, 0], angle),
        };
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2, bottom: b[1], top: b[3], ..Params::ZERO };
        let mut body = Player::spawn(self.x + offset, self.y);
        if !boss.separated || changed {
            // It sleeps where it was authored; the first step out resolves it
            // from any terrain it was placed into, and so does every frame
            // whose box grows into terrain (Box2D pushes a resized collider out).
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) {
                if !boss.separated { self.spawn_state = SpawnState::Blocked; return; }
            }
            boss.separated = true;
            self.spawn_state = SpawnState::Ready;
        }
        let start = [body.x, body.y];
        body.vy = v[1];
        body.step(p, v[0].signum(), false, count, edge);
        let moved = [body.x - start[0], body.y - start[1]];
        let blocked_x = moved[0] != v[0] / 60;
        let blocked_y = moved[1] != v[1] / 60;
        self.x = body.x - offset;
        self.y = body.y;
        let bonk = if blocked_y && v[1] > 0 { Some(Side::Up) }
            else if blocked_x && v[0] > 0 { Some(Side::Right) }
            else if blocked_y && v[1] < 0 { Some(Side::Down) }
            else if blocked_x && v[0] < 0 { Some(Side::Left) }
            else { None };
        let actions = boss.controller.tick(gz::Senses { position: [self.x, self.y], hero, hero_in_range: false, bonk, landed: false });
        self.apply_gruz(boss, actions, emit);
    }
    /// The corpse hangs where the body died (its prefab has no Rigidbody2D);
    /// the burster it blows out falls on gravity 1 with ObjectBounce 0.5.
    fn advance_gruz_dead(&mut self, boss: &mut GruzRuntime, count: usize,
                         edge: &impl Fn(usize) -> [i32; 4], emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::gruz_mother::{self as gz, Motion};
        boss.tick = boss.tick.saturating_add(1);
        let arena = boss.arena.tick();
        self.apply_battle_scene(arena);
        if boss.rumble && boss.tick % 30 == 1 { crate::camera::request(crate::camera::Shake::Small); }
        let phase = boss.controller.phase();
        if matches!(phase, gz::Phase::Gone | gz::Phase::Spawned) { return; }
        let still = gz::Senses { position: [self.x, self.y], ..gz::Senses::default() };
        if !boss.controller.burster() {
            let actions = boss.controller.tick(still);
            self.apply_gruz(boss, actions, emit);
            return;
        }
        let (frame, changed) = boss.frame_box(crate::gruz_art::GZ_BURSTER_BOX);
        let b = boss.body(frame);
        let Motion::Vector(v) = boss.controller.motion() else { return };
        let offset = b[0] + (b[2] - b[0]) / 2;
        let p = Params { speed: v[0].abs(), gravity: gz::BURSTER_GRAVITY, fall: 100 * ONE, half_width: (b[2] - b[0]) / 2,
            bottom: b[1], top: b[3], ..Params::ZERO };
        let mut body = Player::spawn(self.x + offset, self.y);
        if !boss.separated || changed {
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, edge) && !boss.separated { return; }
            boss.separated = true;
        }
        let start = [body.x, body.y];
        body.vy = v[1];
        body.grounded = boss.grounded && v[1] <= 0;
        body.step(p, v[0].signum(), false, count, edge);
        let moved_x = body.x - start[0];
        let blocked_x = moved_x != v[0] / 60;
        self.x = body.x - offset;
        self.y = body.y;
        // ObjectBounce: an entering contact faster than the threshold reflects
        // off it and keeps `bounceFactor` of the speed; slower, the contact
        // stops that axis (Box2D's friction ends the slide on the floor).
        let speed = distance_q16(v[0], v[1]);
        let fast = speed > gz::BURSTER_BOUNCE_THRESHOLD;
        let half = |value: i32| ((value as i64 * gz::BURSTER_BOUNCE as i64) >> 16) as i32;
        let landed = body.grounded && !boss.grounded;
        let mut next = [v[0], body.vy];
        if landed {
            next = if fast { [half(v[0]), -half(v[1])] } else { [0, 0] };
        } else if body.grounded {
            next = [0, 0];
        }
        if blocked_x {
            next[0] = if fast { -half(v[0]) } else { 0 };
        }
        boss.grounded = body.grounded && next[1] <= 0;
        boss.controller.set_velocity(next);
        let actions = boss.controller.tick(gz::Senses { landed: body.grounded, ..still });
        self.apply_gruz(boss, actions, emit);
    }
    /// The nail or a spell against the body. `Invincible` refuses it (the
    /// hero is not yet in `Battle Range`); asleep the first hit wakes it.
    #[inline(never)]
    fn strike_gruz(&mut self, boss: &mut GruzRuntime, spec: &ActorSpec, hits: &dyn Fn([i32; 4]) -> bool,
                   damage: u16) -> (bool, Hit) {
        let (frame, _) = boss.frame_box(spec.bounds);
        let b = boss.body(frame);
        let sink = if boss.controller.sunk() { hk_sim::gruz_mother::SLAM_SINK } else { 0 };
        if !hits([self.x + b[0], self.y + b[1] - sink, self.x + b[2], self.y + b[3] - sink]) {
            return (false, Hit::Ignored);
        }
        let params = hk_sim::EnemyParams { health: hk_sim::gruz_mother::HEALTH, contact_damage: 0,
            evasion_ticks: hk_sim::gruz_mother::INVULNERABLE_TICKS, invincible: boss.controller.invincible(),
            damage_override: false };
        let hit = self.health.hit(params, damage);
        if matches!(hit, Hit::Damaged | Hit::Killed) {
            self.flash_left = FLASH_TICKS;
            unsafe { HK_GZ_HITS = HK_GZ_HITS.saturating_add(1) }
            let actions = if hit == Hit::Killed { boss.controller.die() } else { boss.controller.took_damage() };
            self.apply_gruz(boss, actions, &mut no_runner_event);
        }
        (true, hit)
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
        boss.head_tick = boss.head_tick.saturating_add(1);
        if boss.death_head != 0 {
            boss.death_head_tick = boss.death_head_tick.saturating_add(1);
        }
        if boss.death_head == 1 {
            // The Death Head leaves the armour at floor height, so it slides
            // rather than falls: a wall a unit up stops it.
            let [x, y] = boss.death_head_at(self.x, self.y);
            let step = if boss.controller.facing_right() { 1 } else { -1 } * crate::fk_art::FK_DEATH_HEAD_SPEED / 60;
            if segment_hits_terrain([x, y + ONE], [x + step, y + ONE], count, edge) {
                boss.death_head = 2;
            } else {
                boss.death_head_travel += step;
            }
        }
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
        let wave = actions.iter().find_map(|a| match a { fk::Action::Shockwave { right, .. } => Some(right), _ => None });
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
        // `S Attack Recover` spawns the wave at `Shockwave X Origin` ahead of
        // the body and `FK_WAVE_ORIGIN_Y` below its transform, in world units:
        // SpawnObjectFromGlobalPool adds the offset to the spawn point unscaled.
        if let Some(right) = wave {
            let dir = if right { 1 } else { -1 };
            emit(RunnerEvent { scene: self.scene, source_id: self.source_id,
                position: [self.x + dir * fk::SHOCKWAVE_X_ORIGIN, self.y + crate::fk_art::FK_WAVE_ORIGIN_Y],
                facing: dir, kind: RunnerEventKind::Shockwave });
        }
    }
    /// One 60 Hz step of the Baldur: FSM tick from last frame's contacts, then
    /// the gravity body (scale 0.8) moves at the held x velocity and the
    /// Recoil displacement, and this frame's wall/ground contacts are kept.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
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
    /// One 60 Hz step of the Blocker, which is three box overlaps and a clip.
    ///
    /// `CheckAlertRangeByName` reads an `AlertRange` child, and `AlertRange`
    /// is driven by `PlayMakerTriggerStay` on the child's own trigger box, so
    /// the sense is the hero's collider against that box. All three boxes are
    /// `crate::blocker` constants in the actor's own frame; they are never
    /// mirrored by facing, because the placement's transform mirror does not
    /// reach this FSM at all and `host/blocker.py` proves the authored boxes
    /// against those constants before a placement is admitted.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_blocker(&mut self, blocker: &mut BlockerRuntime, spec: &ActorSpec, room: &Room,
        context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::blocker::Action;
        blocker.animation_tick = blocker.animation_tick.saturating_add(1);
        let at = |b: [i32; 4]| overlap(
            [self.x + b[0], self.y + b[1], self.x + b[2], self.y + b[3]], context.hero_body);
        let senses = hk_sim::blocker::Senses {
            in_alert_range: at(hk_sim::blocker::ALERT),
            in_attack_range: at(hk_sim::blocker::ATTACK),
            in_unalert_range: at(hk_sim::blocker::UNALERT),
            completed: blocker.completed(spec, room),
            can_roller: crate::blocker_roller::can_spawn(),
        };
        let actions = blocker.controller.tick(senses, |r| BlockerRuntime::sample(&mut blocker.rng, r));
        let ActorController::Blocker { shot_clip, impact_clip, .. } = spec.controller
            else { panic!("Blocker runtime with other metadata") };
        for action in actions.iter() {
            match action {
                Action::Play(_) => blocker.animation_tick = 0,
                Action::Fire { offset, velocity } => emit(RunnerEvent {
                    scene: self.scene, source_id: self.source_id, position: [self.x, self.y],
                    facing: self.walk.direction,
                    kind: RunnerEventKind::Fire { velocity, offset, goop: true, shot_clip, impact_clip } }),
                Action::Roller { offset, velocity } => emit(RunnerEvent {
                    scene: self.scene, source_id: self.source_id, position: [self.x, self.y],
                    facing: self.walk.direction, kind: RunnerEventKind::Roller { velocity, offset } }),
            }
        }
    }
    // Its own function so the family can stream as a code module.
    #[inline(never)]
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
                    facing: self.walk.direction, kind: RunnerEventKind::Fire { velocity, offset: [0; 2],
                        goop: false, shot_clip, impact_clip } }),
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
    /// The gravity-free dynamic body the Hatcher and its babies share with the
    /// Aspid: the controller owns the velocity and the bounded solver carries it
    /// against the resident terrain, then the Recoil displacement if one is due.
    fn fly_body(&mut self, spec: &ActorSpec, v: [i32; 2], count: usize, edge: &impl Fn(usize) -> [i32; 4]) {
        let b = spec.bounds;
        let offset = b[0] + (b[2] - b[0]) / 2;
        let mut body = Player::spawn(self.x + offset, self.y);
        let mut p = Params { speed: v[0].abs(), fall: 100 * ONE, half_width: (b[2] - b[0]) / 2,
            bottom: b[1], top: b[3], ..Params::ZERO };
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
    /// One 60 Hz step of the Hatcher: the `alert_range` sense (the 7.804 circle
    /// and an unobstructed line), the FSM tick, then the shared flying body.
    /// A `Release` leaves as an event because the baby that answers it is
    /// another actor in the same pool, which this borrow cannot reach.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_hatcher(&mut self, hatcher: &mut HatcherRuntime, spec: &ActorSpec, count: usize,
        edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext, emit: &mut impl FnMut(RunnerEvent)) {
        use hk_sim::hatcher::Action;
        use hk_sim::runner_senses as q;
        hatcher.animation_tick = hatcher.animation_tick.saturating_add(1);
        let pos = [self.x, self.y];
        let hb = context.hero_body;
        let dx = (pos[0].clamp(hb[0], hb[2]) - pos[0]) as i64;
        let dy = (pos[1].clamp(hb[1], hb[3]) - pos[1]) as i64;
        const ALERT: i64 = hk_sim::hatcher::ALERT_RADIUS as i64;
        let in_alert = dx * dx + dy * dy <= ALERT * ALERT;
        let clear = match q::line_of_sight(pos, context.hero, true, count, edge) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Hatcher LOS outside validated coordinates"),
        };
        let senses = hk_sim::hatcher::Senses { position: pos, hero: context.hero,
            alert_range: in_alert && clear, cage_children: context.cage_children };
        for action in hatcher.controller.tick(senses).iter() {
            match action {
                Action::Play(clip, tick) => { hatcher.clip = clip; hatcher.animation_tick = tick; }
                Action::Facing(f) => self.walk.direction = f,
                Action::Release { position, velocity } => emit(RunnerEvent { scene: self.scene,
                    source_id: self.source_id, position, facing: self.walk.direction,
                    kind: RunnerEventKind::Release { velocity } }),
                Action::Velocity(_) => {}
            }
        }
        let v = hatcher.controller.velocity();
        self.fly_body(spec, v, count, edge);
    }
    /// One 60 Hz step of a released baby. A parked one never reaches here: the
    /// tick skips it before the terrain its cage sits outside is even resolved.
    // Its own function so the family can stream as a code module.
    #[inline(never)]
    fn advance_baby(&mut self, baby: &mut BabyRuntime, spec: &ActorSpec, count: usize,
        edge: &impl Fn(usize) -> [i32; 4], context: RunnerContext) {
        use hk_sim::hatcher::BabyAction;
        baby.animation_tick = baby.animation_tick.saturating_add(1);
        for action in baby.controller.tick([self.x, self.y], context.hero).iter() {
            match action {
                BabyAction::Facing(f) => self.walk.direction = f,
                BabyAction::Velocity(_) => {}
            }
        }
        let v = baby.controller.velocity();
        self.fly_body(spec, v, count, edge);
    }
    fn runner_senses(&self, runner:&RunnerRuntime, spec:&ActorSpec, room:&Room,
        context:RunnerContext, count:usize, edge:impl Fn(usize)->[i32;4]) -> hk_sim::runner::Senses {
        use hk_sim::runner_senses as q;
        let pos = [self.x,self.y];
        let ActorController::Runner { alert, .. } = spec.controller else { panic!("Runner senses without Runner metadata") };
        // Body and alert boxes come from the placement (facing-left frame); the
        // level37 Runner shape is `Shape::RUNNER`.
        let shape = q::Shape::from_placement(spec.bounds, alert, self.initial_direction as i32);
        let expected = q::body_bounds_of(shape, pos, runner.controller.facing()).expect("Runner bounds");
        assert_eq!(self.bounds(spec), expected, "unsupported Runner collider variant");
        let in_alert_range = q::alert_overlap_of(shape,pos,context.hero_body).expect("Runner alert bounds");
        let can_see_hero = match q::line_of_sight_near(pos,context.hero,in_alert_range,count,&edge,edges_near) {
            Ok(value) => value,
            // Coincident transform points have no proven Physics2D equivalent;
            // conservatively suppress sight (TOOK DAMAGE still reacts).
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Runner LOS outside validated coordinates"),
        };
        let probes = q::walker_queries_near(shape,pos,runner.controller.facing(),count,edge,edges_near)
            .expect("Runner Sweep outside validated coordinates");
        hk_sim::runner::Senses {
            camera_in_start_range: hk_sim::runner::camera_in_start_range(context.camera,[self.x,self.y,66]),
            hero_x:context.hero[0], actor_x:self.x, in_alert_range, can_see_hero,
            wall:probes.wall, floor_ahead:probes.floor_ahead,
            completed:runner.completed(spec,room), grounded:self.grounded,
        }
    }
    /// The Zombie Shield's senses, which are the Runner's: the same `Walker`
    /// Sweep pair, the same `LineOfSightDetector`, and one `AlertRange` that
    /// both the Walker's turn-to-face-hero and `ZombieShieldControl` read.
    fn shield_senses(&self, shield:&ZombieShieldRuntime, spec:&ActorSpec, room:&Room,
        context:RunnerContext, count:usize, edge:impl Fn(usize)->[i32;4]) -> hk_sim::zombie_shield::Senses {
        use hk_sim::runner_senses as q;
        let pos = [self.x,self.y];
        let ActorController::ZombieShield { attack, .. } = spec.controller
            else { panic!("Zombie Shield senses without Zombie Shield metadata") };
        let shape = q::Shape::from_placement(spec.bounds, attack, self.initial_direction as i32);
        let facing = shield.controller.facing();
        let expected = q::body_bounds_of(shape, pos, facing).expect("Zombie Shield bounds");
        assert_eq!(self.bounds(spec), expected, "unsupported Zombie Shield collider variant");
        let in_attack_range = q::alert_overlap_of(shape,pos,context.hero_body).expect("Zombie Shield alert bounds");
        let can_see_hero = match q::line_of_sight_near(pos,context.hero,in_attack_range,count,&edge,edges_near) {
            Ok(value) => value,
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Zombie Shield LOS outside validated coordinates"),
        };
        let probes = q::walker_queries_near(shape,pos,facing,count,edge,edges_near)
            .expect("Zombie Shield Sweep outside validated coordinates");
        hk_sim::zombie_shield::Senses {
            camera_in_start_range: hk_sim::runner::camera_in_start_range(context.camera,[self.x,self.y,66]),
            position: pos, hero: context.hero, in_attack_range, can_see_hero,
            wall: probes.wall, floor_ahead: probes.floor_ahead,
            completed: shield.completed(spec,room),
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
    /// Live `Shockwave Wave`s. A slam makes one and the next slam cannot come
    /// before the wave has crossed the arena, so two is headroom, not a guess
    /// at load: a third evicts the oldest, the pool's own rule.
    waves: [Option<(u8, hk_sim::shockwave::Wave, u16)>; WAVES],
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
    /// `world::actors_key` of the region `sync_region` last seated in full,
    /// and for each actor slot its placement there: the object index
    /// `world::region_actor` reads (NO_PLACEMENT if none) and its spec. A
    /// seated region stays seated until `reset_scene` empties slots, so the
    /// tick and the draw read these instead of searching the region's
    /// placements per actor.
    placed_key: Option<usize>,
    placement: [u8; MAX_ACTORS],
    placed_spec: [Option<&'static ActorSpec>; MAX_ACTORS],
}
const NO_PLACEMENT: u8 = u8::MAX;
impl EnemyWorld {
    /// Source shots are pooled per Aspid (two each) and per Blocker (two goop
    /// and one roller); the oldest live shot yields when the shared pool is
    /// full.
    fn spawn_shot(&mut self, scene: usize, position: [i32; 2], velocity: [i32; 2], kind: ShotKind,
        shot_clip: u16, impact_clip: u16) {
        self.spawn_projectile(Shot { scene: scene as u8, x: position[0], y: position[1], vx: velocity[0],
            vy: velocity[1], animation_tick: 0, impact: false, kind, shot_clip, impact_clip });
    }
    /// `summon`'s `Spawn`: one `Falling Barrel` at the chosen x and the
    /// summoner's own y, at rest, with the pool's own recycling rule.
    /// `spurt` is a Husk Guard wave's spurt clip in its room, or NO_SPURT for
    /// the False Knight's, whose art is its own bank (`crate::fk_art`).
    fn spawn_wave(&mut self, scene: usize, position: [i32; 2], dir: i32, spurt: u16) {
        let wave = hk_sim::shockwave::Wave::new(position, dir, wave_params(spurt));
        // A third wave evicts the oldest, the pool's own rule.
        let slot = self.waves.iter().position(Option::is_none).unwrap_or_else(|| {
            let mut oldest = 0;
            for (i, w) in self.waves.iter().enumerate() {
                if w.map_or(0, |(_, w, _)| w.age()) >= self.waves[oldest].map_or(0, |(_, w, _)| w.age()) { oldest = i; }
            }
            oldest
        });
        self.waves[slot] = Some((scene as u8, wave, spurt));
        unsafe { HK_FK_WAVES = HK_FK_WAVES.saturating_add(1) }
    }
    /// `shockwave`'s `Move` and the spurts it leaves: see `hk_sim::shockwave`.
    fn tick_waves(&mut self, region: &Region, hero_body: [i32; 4], count: usize, edge: &impl Fn(usize) -> [i32; 4],
        mut hurt: impl FnMut(u16, i32) -> Hurt) -> Hurt {
        let mut result = Hurt::Ignored;
        for slot in self.waves.iter_mut() {
            let Some((scene, wave, spurt)) = slot else { continue };
            let p = wave_params(*spurt);
            if *scene as usize != region.scene { continue; }
            let done = wave.step(p, |a, b| segment_hits_terrain(a, b, count, edge));
            if wave.hurts(p, hero_body) {
                let outcome = hurt(p.damage, wave.dir as i32);
                if outcome != Hurt::Ignored {
                    result = outcome;
                    unsafe { HK_FK_WAVE_HITS = HK_FK_WAVE_HITS.saturating_add(1) }
                }
            }
            if done { *slot = None; }
        }
        result
    }
    fn spawn_barrel(&mut self, scene: usize, position: [i32; 2], clip: u16) {
        self.spawn_projectile(Shot { scene: scene as u8, x: position[0], y: position[1], vx: 0, vy: 0,
            animation_tick: 0, impact: false, kind: ShotKind::Barrel, shot_clip: clip, impact_clip: clip });
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
        // With more than one shot in flight, read the room's edges once and
        // index them, instead of every shot scanning every edge (the Mawlek's
        // spit volleys). Same edges, same answers (segment_hits_edges).
        let flying = self.shots.iter().flatten().filter(|s| s.scene as usize == region.scene && !s.impact).count();
        let mut copy = [[0i32; 4]; EDGES];
        let index = if flying > 1 && count <= EDGES {
            for (i, e) in copy[..count].iter_mut().enumerate() { *e = edge(i); }
            Some(hk_sim::runner_senses::EdgeColumns::build(&copy[..count]))
        } else { None };
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
            let hit_terrain = match &index {
                Some(index) => segment_hits_edges([ox, oy], [shot.x, shot.y], &copy[..count],
                    index.near([ox.min(shot.x), oy.min(shot.y), ox.max(shot.x), oy.max(shot.y)])),
                None => segment_hits_terrain([ox, oy], [shot.x, shot.y], count, edge),
            };
            let half = shot.half();
            let bounds = [shot.x - half[0], shot.y - half[1], shot.x + half[0], shot.y + half[1]];
            let hit_hero = overlap(bounds, hero_body);
            if hit_hero {
                // A barrel falls straight down, so the recoil side is the side
                // of the hero it landed on rather than the side it came from.
                let outcome = hurt(shot.damage(), if shot.kind == ShotKind::Barrel {
                    if shot.x < (hero_body[0] + hero_body[2]) / 2 { -1 } else { 1 }
                } else if shot.vx < 0 { -1 } else { 1 });
                if outcome != Hurt::Ignored {
                    result = outcome;
                    if shot.kind == ShotKind::Mawlek { unsafe { HK_MW_SHOT_HITS = HK_MW_SHOT_HITS.saturating_add(1) } }
                }
            }
            if hit_terrain || hit_hero {
                if shot.kind == ShotKind::Barrel {
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
    /// The Hatcher's `Fire`, finished: `SetPosition` the cage child it drew,
    /// `SPAWN` it and `SetVelocity2d(y = -5)`. The pool is a cook-time
    /// reservation, so this only ever changes an actor already seated in the
    /// scene, and the controller only asks while `GetChildCount(Cage)` is
    /// positive, so a release cannot fail to find one.
    ///
    /// The source draws uniformly with `GetRandomChild`; the guest takes the
    /// first parked slot, which is a divergence in which baby answers and in
    /// nothing else, because every member of a cage is the same object.
    fn release_baby(&mut self, scene: usize, position: [i32; 2], velocity: [i32; 2]) {
        let Some(actor) = self.actors.iter_mut().flatten()
            .find(|a| a.scene == scene && a.parked_baby()) else { return };
        actor.x = position[0];
        actor.y = position[1];
        actor.vy = velocity[1];
        actor.recoil_left = 0;
        actor.recoil = [0; 2];
        if let Runtime::HatcherBaby(baby) = &mut actor.runtime {
            baby.controller.release(velocity);
            baby.animation_tick = 0;
        }
    }
    /// `Spawn Flies 2`: SetPosition `Fly Spawn` (world space) to the burster,
    /// so each parked reserve fly lands at the burster plus its own authored
    /// offset from `Fly Spawn`, still in `Initialise`: its camera test passes
    /// there, and it aims and flies as any Gruzzer.
    fn release_reserve(&mut self, scene: usize, at: [i32; 2]) {
        for (slot, actor) in self.actors.iter_mut().enumerate() {
            let Some(actor) = actor.as_mut().filter(|a| a.scene == scene && a.parked_reserve()) else { continue };
            let Some(ActorController::GruzzerReserve { origin }) = self.placed_spec[slot].map(|s| s.controller) else { continue };
            actor.x = at[0] + (actor.x - origin[0]);
            actor.y = at[1] + (actor.y - origin[1]);
            actor.located = crate::disc::Located::NONE;
            if let Runtime::Gruzzer(fly) = &mut actor.runtime { fly.parked = false; }
        }
    }
    pub fn clear_shots(&mut self, scene: usize) {
        for slot in self.shots.iter_mut() {
            if slot.is_some_and(|s| s.scene as usize == scene) { *slot = None; }
        }
        for slot in self.waves.iter_mut() {
            if slot.is_some_and(|(id, _, _)| id as usize == scene) { *slot = None; }
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
            waves: [None; WAVES],
            summon: hk_sim::false_knight::Summon::new(1),
            summon_site: None,
            placed_key: None,
            placement: [NO_PLACEMENT; MAX_ACTORS],
            placed_spec: [None; MAX_ACTORS],
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
    /// The first `max` live actors for `HK_TRACE`, `trace::ENEMY_WORDS` words
    /// each: source id; scene, controller kind and flags (bit 0 grounded, 1
    /// dead, 2 hit flash) as `scene << 24 | kind << 16 | flags`; x; y; hit
    /// points (low half) and evasion ticks (high half); vertical velocity;
    /// walk direction; walk animation tick. Returns (live actors, slots written).
    #[cfg(not(test))]
    #[cfg_attr(not(test),optimize(size))]
    pub fn trace(&self, out: &mut [u32], max: usize) -> (u32, u32) {
        let (mut live, mut written) = (0u32, 0usize);
        for a in self.actors.iter().flatten() {
            live += 1;
            if written >= max { continue; }
            let w = &mut out[written * crate::trace::ENEMY_WORDS..][..crate::trace::ENEMY_WORDS];
            let kind = match a.runtime {
                Runtime::Walker => 0u32, Runtime::Runner(_) => 1, Runtime::Climber(_) => 2, Runtime::Vengefly(_) => 3,
                Runtime::Gruzzer(_) => 4, Runtime::AcidFlyer(_) => 5, Runtime::Mosquito(_) => 6, Runtime::MossWalker(_) => 7,
                Runtime::Baldur(_) => 8, Runtime::Aspid(_) => 9, Runtime::Hatcher(_) => 10, Runtime::HatcherBaby(_) => 11,
                Runtime::ZombieShield(_) => 12, Runtime::HuskGuard(_) => 13, Runtime::Blocker(_) => 14, Runtime::Pigeon(_) => 15,
                Runtime::Static { .. } => 16, Runtime::FalseKnight(_) => 17, Runtime::Mawlek(_) => 18, Runtime::GruzMother(_) => 19,
            };
            let flags = a.grounded as u32 | (a.health.dead as u32) << 1 | ((a.flash_left > 0) as u32) << 2;
            w[0] = a.source_id;
            w[1] = (a.scene as u32) << 24 | kind << 16 | flags;
            w[2] = a.x as u32;
            w[3] = a.y as u32;
            w[4] = a.health.hp as u16 as u32 | (a.health.evasion_ticks as u32) << 16;
            w[5] = a.vy as u32;
            w[6] = a.walk.direction as u32;
            w[7] = a.walk.animation_tick;
            written += 1;
        }
        for w in out[written * crate::trace::ENEMY_WORDS..max * crate::trace::ENEMY_WORDS].iter_mut() { *w = 0; }
        (live, written as u32)
    }
    pub fn reset_scene(&mut self, scene: usize) {
        for slot in &mut self.actors {
            if slot.as_ref().is_some_and(|a| a.scene == scene) {
                *slot = None;
            }
        }
        self.placed_key = None;
        self.clear_shots(scene);
        // A death or a gate leaves the arena: no roar or arena end outlives it.
        unsafe { ROAR_LOCK = false; MW_PIECE_WAIT = false; }
    }
    #[cfg_attr(not(test),optimize(size))]
    pub fn sync_region(&mut self, region: &Region) {
        let key = crate::world::actors_key(region);
        if key.is_some() && key == self.placed_key {
            return;
        }
        for (placement, spec) in crate::world::region_actors(region) {
            if self
                .actors
                .iter()
                .flatten()
                .any(|a| a.scene == region.scene && a.source_id == placement.source_id)
            {
                continue;
            }
            let slot = self
                .actors
                .iter_mut()
                .find(|a| a.is_none())
                .expect("enemy source pool exceeds 32");
            *slot = Some(Actor::new(region.scene, &placement, spec));
            // A Blocker whose `PersistentBoolItem` is set was destroyed with its
            // Terrain Block; seat it dead so it neither draws nor answers.
            if crate::blocker_terrain::dead(region.scene, placement.source_id) {
                let a = slot.as_mut().expect("the actor just seated");
                a.health.dead = true;
                a.geo_paid = true;
                if let Runtime::Blocker(b) = &mut a.runtime { b.controller.die(); }
            }
            // Any other enemy whose source `PersistentBoolItem` kept its death:
            // `HealthManager` deactivates it on load, so it is seated dead, with
            // no corpse to draw, and its Geo was paid when it fell.
            if crate::actor_persistence::dead(region.scene, placement.source_id) {
                let a = slot.as_mut().expect("the actor just seated");
                a.health.hp = 0;
                a.health.dead = true;
                a.geo_paid = true;
            }
            // A boss reports its load-time state before anything advances it,
            // so a route that never crosses the trigger still reads 65 hp and a
            // dormant fight rather than the zeros a fresh session starts with.
            // A won Mawlek arena's `Activate` destroyed the body: seated dead,
            // it neither draws nor answers, and its arena stays open.
            let seated = slot.as_mut().expect("the actor just seated");
            if seated.mawlek().is_some_and(|m| m.controller.phase() == hk_sim::mawlek::Phase::Gone)
                || seated.gruz().is_some_and(|g| g.controller.phase() == hk_sim::gruz_mother::Phase::Gone) {
                seated.health.dead = true;
                seated.geo_paid = true;
            }
            if let Some(boss) = seated.false_knight() { seated.publish_false_knight(boss); }
            if let Some(boss) = seated.mawlek() { seated.publish_mawlek(boss); }
            if let Some(boss) = seated.gruz() { seated.publish_gruz(boss); }
        }
        // Each slot's first placement in listing order with its source id,
        // which is what a search of the region's placements would find.
        self.placement = [NO_PLACEMENT; MAX_ACTORS];
        self.placed_spec = [None; MAX_ACTORS];
        for (index, placement, spec) in crate::world::region_actors_indexed(region) {
            for (slot, actor) in self.actors.iter().enumerate() {
                if actor.as_ref().is_some_and(|a| a.scene == region.scene && a.source_id == placement.source_id)
                    && self.placed_spec[slot].is_none()
                {
                    assert!(index < NO_PLACEMENT as usize, "placement index past the table");
                    self.placement[slot] = index as u8;
                    self.placed_spec[slot] = Some(spec);
                }
            }
        }
        self.placed_key = key;
    }
    /// The spec of the slot's placement in `region`, from the table when it
    /// was built for `region`, else by searching the region's placements.
    fn spec_in(&self, slot: usize, region: &Region, source_id: u32) -> Option<&'static ActorSpec> {
        if self.placed_key.is_some() && self.placed_key == crate::world::actors_key(region) {
            return self.placed_spec[slot];
        }
        crate::world::region_actors(region).find(|(p, _)| p.source_id == source_id).map(|(_, spec)| spec)
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
        resident: impl Fn(usize, i32, i32, &mut crate::disc::Located) -> Option<(Region, Room<'a>)>,
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
        let mut context=RunnerContext {camera,hero:[player.x,player.y],hero_body:body,cage_children:0};
        // `GetChildCount(Cage)`: the scene's parked cage members. One pass over
        // the pool, because the cage is scene-wide and a Hatcher only fires
        // while something is still in it.
        let parked = core::cell::Cell::new(self.actors.iter().flatten()
            .filter(|a| a.scene == region.scene && a.parked_baby()).count() as u16);
        // Aspid and Blocker shots spawn after the actor pass (the pool is a
        // sibling field).
        let mut fires: [Option<(usize, [i32; 2], [i32; 2], ShotKind, u16, u16)>; 4] = [None; 4];
        // The same deferral for a release: the baby that answers one is another
        // actor in the pool this loop already holds.
        let mut releases: [Option<([i32; 2], [i32; 2])>; RELEASES] = [None; RELEASES];
        // At most one SUMMON a tick: one boss, and `Actions` carries one of
        // these per state change.
        let mut summoned: Option<(usize, u8, i32, u16)> = None;
        let mut waved: [Option<(usize, [i32; 2], i32, u16)>; 3] = [None; 3];
        let mut effects: [Option<(usize, [i32; 2], i32, u16)>; 2] = [None; 2];
        // The Mawlek's spit and its Head's shot can land on the same tick.
        let mut sprays: [Option<(usize, [i32; 2], u8, [i32; 2], [i32; 2], u32)>; 2] = [None; 2];
        // Gruz Mother's burster releasing its reserve, and reserve flies that
        // died this tick (each one decrements the arena's `Battle Enemies`).
        let mut reserve_release: Option<[i32; 2]> = None;
        let mut reserve_deaths: u8 = 0;
        let mut runner_event = |event: RunnerEvent| {
            if let RunnerEventKind::Fire { velocity, offset, goop, shot_clip, impact_clip } = event.kind {
                if let Some(slot) = fires.iter_mut().find(|f| f.is_none()) {
                    let origin = [event.position[0] + offset[0], event.position[1] + offset[1]];
                    let kind = if goop { ShotKind::Goop } else { ShotKind::Spit };
                    // `spitter` Fire's one shot; the Blocker's goop has its own.
                    if !goop { crate::scene_sfx::play(crate::scene_sfx::ASPID_SPIT); }
                    *slot = Some((event.scene, origin, velocity, kind, shot_clip, impact_clip));
                }
            } else if let RunnerEventKind::Summon { spawns, spawn_y, barrel_clip } = event.kind {
                summoned = Some((event.scene, spawns, spawn_y, barrel_clip));
            } else if let RunnerEventKind::ReleaseReserve = event.kind {
                reserve_release = Some(event.position);
            } else if let RunnerEventKind::MawlekShots { count, speed, angles, seed } = event.kind {
                if let Some(slot) = sprays.iter_mut().find(|s| s.is_none()) {
                    *slot = Some((event.scene, event.position, count, speed, angles, seed));
                }
            } else if let RunnerEventKind::Shockwave = event.kind {
                if let Some(slot) = waved.iter_mut().find(|w| w.is_none()) {
                    *slot = Some((event.scene, event.position, event.facing, NO_SPURT));
                }
            } else if let RunnerEventKind::GuardWave { spurt_clip } = event.kind {
                if let Some(slot) = waved.iter_mut().find(|w| w.is_none()) {
                    *slot = Some((event.scene, event.position, event.facing, spurt_clip));
                }
            } else if let RunnerEventKind::Effect { clip } = event.kind {
                if let Some(slot) = effects.iter_mut().find(|w| w.is_none()) {
                    *slot = Some((event.scene, event.position, event.facing, clip));
                }
            } else if let RunnerEventKind::Release { velocity } = event.kind {
                // The controller only fires while it can see a parked baby, so
                // this take is the one the cage just lost. host/hatcher.py
                // refuses a scene with more than RELEASES Hatchers, which is
                // what makes the slot search below always find one.
                parked.set(parked.get().saturating_sub(1));
                let slot = releases.iter_mut().find(|r| r.is_none());
                debug_assert!(slot.is_some(), "more Hatchers fired on one frame than the cook admits");
                if let Some(slot) = slot { *slot = Some((event.position, velocity)); }
            } else { runner_event(event); }
        };
        // sync_region above seated this region, so the table is its own.
        debug_assert!(self.placed_key.is_none() || self.placed_key == crate::world::actors_key(region));
        let (placement_of, spec_of) = (self.placement, self.placed_spec);
        for (slot, actor) in self
            .actors
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, a)| Some((slot, a.as_mut()?)))
            .filter(|(_, a)| a.scene == region.scene)
        {
            let Some(spec) = spec_of[slot] else {
                continue;
            };
            if actor.baby().is_some() {
                // `Death` recycles instead of removing, on the frame after the
                // one the kill was counted on: a dead baby has no corpse, so it
                // draws nothing and its contact damage is already zero there.
                if actor.health.dead {
                    let (placement, _) = crate::world::region_actor(region, placement_of[slot] as usize)
                        .expect("a seated slot's placement");
                    actor.park_baby(&placement, spec);
                }
                // A parked baby is `Inert` inside a cage the source parks off
                // the map. It has no senses, no body and no draw, so it stops
                // here rather than paying the resident-view lookup below.
                if actor.parked_baby() { continue; }
            }
            // A reserve fly waiting under the room is `Fly Spawn`'s, which the
            // source parks off the map: no senses, no body, no draw.
            if actor.parked_reserve() { continue; }
            // `Destroy`'s `DestroySelf` removed the source object, so a bird
            // that has flown does not advance, draw or answer the nail. It is
            // taken here rather than in `advance` because the nail pass below
            // would otherwise keep hitting a body nothing can see.
            if actor.pigeon().is_some_and(|b| b.controller.phase() == hk_sim::pigeon::Phase::Gone) {
                continue;
            }
            // WalkLeftRight keeps running independently of the Knight/camera.
            // A cooked view boundary is not a source scene or FSM boundary.
            let (x,y)=actor.corpse.map_or((actor.x,actor.y),|c|(c.x,c.y));
            // ponytail: with a whole scene resident, only actors within one view of
            // the active view advance; farther ones hold their state (source Unity
            // keeps simulating them). Widen or drop when a scene needs it.
            let b=region.bounds;
            // The False Knight's last jump breaks the arena floor and it falls
            // two dozen units into the room below, out of range of a hero still
            // standing on what is left: it keeps falling there, as it does in
            // the source, rather than hanging where the range ends.
            let near=crate::world::contains([b[0]-NEAR[0],b[1]-NEAR[1],b[2]+NEAR[0],b[3]+NEAR[1]],x,y)
                || actor.false_knight().is_some_and(|boss| boss.controller.phase()==hk_sim::false_knight::Phase::DeathFall);
            // A far actor never advances, so its view is looked up only if the
            // nail reaches it below; the lookup has no side effects to reorder.
            let in_view=crate::world::contains(region.bounds,x,y);
            let deferred=!in_view && !near;
            // After a view change every near off-view actor can miss its
            // cached region in the same tick, and each miss walks the bank:
            // service the pad before each lookup so the pass never spans a
            // VBlank without a poll.
            let mut other=if in_view || deferred {None}
                else {
                    #[cfg(not(test))]
                    crate::input::checkpoint();
                    resident(region.scene,x,y,&mut actor.located)
                };
            let (terrain_region,terrain_room)=other.as_ref()
                .map_or((region,room),|(r,room)|(r,room));
            let available=near && terrain_region.scene==region.scene
                && crate::world::contains(terrain_region.collision_bounds,x,y);
            if !actor.health.dead {actor.health.tick();}
            actor.flash_left = actor.flash_left.saturating_sub(1);
            if available {
                context.cage_children=parked.get();
                actor.advance_in_view(spec,state,region,room,terrain_region,terrain_room,context,&mut runner_event);
            }
            // HealthManager.Die's `Battle Enemies` decrement, once per fly.
            if let Runtime::Gruzzer(fly) = &mut actor.runtime {
                if fly.reserve && actor.health.dead && !fly.reported {
                    fly.reported = true;
                    reserve_deaths = reserve_deaths.saturating_add(1);
                }
            }
            if actor.health.dead {continue;}
            if (actor.runner().is_some() || actor.zombie_shield().is_some())
                && (!available || actor.spawn_state==SpawnState::Blocked) {
                continue; // no attack/recoil sensing against an unavailable terrain owner
            }
            let bounds = actor.bounds(spec);
            // Gruz Mother answers the nail through its own box and phases, and
            // hurts from `Hero Damager` rather than its body.
            if let Some(mut boss) = actor.gruz() {
                let mut reached = false;
                if let Some((box_, damage)) = ball {
                    let (_, hit) = actor.strike_gruz(&mut boss, spec, &|b| overlap(box_, b), damage);
                    if matches!(hit, Hit::Damaged | Hit::Killed) { events.hits += 1; }
                    if hit == Hit::Killed { events.kills += 1; }
                }
                if !polygon.is_empty() && !actor.health.dead {
                    let nail_damage = cheats.params(crate::VITAL_PARAMS).nail_damage;
                    let (hit_box, hit) = actor.strike_gruz(&mut boss, spec,
                        &|b| hk_sim::polygon_hits_box(polygon, b), nail_damage);
                    reached = hit_box;
                    if matches!(hit, Hit::Damaged | Hit::Killed) {
                        events.hits += 1;
                        vitals.gain_soul_on_nail_hit(cheats.params(crate::VITAL_PARAMS));
                    }
                    if hit == Hit::Killed { events.kills += 1; }
                }
                if reached && vitals.can_control() {
                    response.contact(crate::NAIL_RESPONSE_PARAMS, nail.kind, player.facing, player);
                }
                let (frame, _) = boss.frame_box(spec.bounds);
                let b = boss.body(frame);
                let hurt_box = [actor.x + b[0], actor.y + b[1], actor.x + b[2], actor.y + b[3]];
                if !dream_polygon.is_empty() && spec.dream_soul != 0 && !actor.dream_taken
                    && hk_sim::polygon_hits_box(dream_polygon, hurt_box) {
                    actor.dream_taken = true;
                    events.dream_soul = events.dream_soul.saturating_add(spec.dream_soul);
                }
                if !actor.health.dead && boss.controller.hurts() {
                    let d = boss.body(crate::gruz_art::GZ_DAMAGER_BOX);
                    if overlap(body, [actor.x + d[0], actor.y + d[1], actor.x + d[2], actor.y + d[3]]) {
                        let direction = if player.x < actor.x { -1 } else { 1 };
                        let hurt = cheats.hurt(vitals, crate::VITAL_PARAMS, hk_sim::gruz_mother::CONTACT_DAMAGE, direction,
                            false, player.shadow_dashing);
                        if hurt != Hurt::Ignored { events.hurt = hurt; }
                    }
                }
                actor.seat_gruz(boss);
                continue;
            }
            // The Mawlek answers through one HealthManager reached by two boxes,
            // clashes with the nail on a live swipe, and hurts from its body
            // and from each swiping arm.
            if let Some(mut boss) = actor.mawlek() {
                let mut reached = false;
                // EnemyDeathEffects' corpse fling, away from the hero: 60
                // degrees for a side hit, straight for up and down.
                let away = if actor.x >= player.x { 1 } else { -1 };
                let speed = crate::mawlek_art::MW_CORPSE_FLING;
                let fling = |kind: u16| match kind {
                    2 => [0, speed],
                    3 => [0, -speed],
                    _ => [away * (speed >> 1), ((speed as i64 * 56756) >> 16) as i32],
                };
                if let Some((box_, damage)) = ball {
                    let (_, hit) = actor.strike_mawlek(&mut boss, spec, &|b| overlap(box_, b), damage, fling(0));
                    if matches!(hit, Hit::Damaged | Hit::Killed) { events.hits += 1; }
                }
                if !polygon.is_empty() {
                    if !actor.health.dead {
                        let nail_damage = cheats.params(crate::VITAL_PARAMS).nail_damage;
                        let (hit_box, hit) = actor.strike_mawlek(&mut boss, spec,
                            &|b| hk_sim::polygon_hits_box(polygon, b), nail_damage, fling(nail.kind));
                        reached = hit_box;
                        if matches!(hit, Hit::Damaged | Hit::Killed) {
                            events.hits += 1;
                            vitals.gain_soul_on_nail_hit(cheats.params(crate::VITAL_PARAMS));
                        }
                    }
                    // `nail_clash_tink`: the slash entering a live swipe
                    // collider parries it once, with the recoil a wall gives.
                    for arm in 0..2 {
                        let arm_box = MawlekRuntime::relative(crate::mawlek_art::MW_ARM_HITBOX[arm], actor.x, actor.y);
                        if boss.arm_hitbox[arm] && !boss.parried[arm] && hk_sim::polygon_hits_box(polygon, arm_box) {
                            boss.parried[arm] = true;
                            reached = true;
                            crate::camera::request(crate::camera::Shake::Kill);
                            crate::scene_sfx::play(crate::scene_sfx::HERO_PARRY);
                            unsafe { HK_MW_PARRIES = HK_MW_PARRIES.saturating_add(1) }
                        }
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
                if !actor.health.dead {
                    let mut damage = 0;
                    if overlap(body, bounds) { damage = spec.health.contact_damage; }
                    for arm in 0..2 {
                        let arm_box = MawlekRuntime::relative(crate::mawlek_art::MW_ARM_HITBOX[arm], actor.x, actor.y);
                        if boss.arm_hitbox[arm] && overlap(body, arm_box) { damage = hk_sim::mawlek::CONTACT_DAMAGE; }
                    }
                    if damage != 0 {
                        let direction = if player.x < actor.x { -1 } else { 1 };
                        let hurt = cheats.hurt(vitals, crate::VITAL_PARAMS, damage, direction, false, player.shadow_dashing);
                        if hurt != Hurt::Ignored { events.hurt = hurt; }
                    }
                }
                actor.seat_mawlek(boss);
                continue;
            }
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
                    // A shut Blocker's shell turns the ball the way it turns
                    // the nail: `IsBlockingByDirection` answers for every
                    // cardinal while `SetInvincible` is up.
                    let params = match actor.blocker() {
                        Some(blocker) => hk_sim::EnemyParams { invincible: blocker.controller.invincible(), ..spec.health },
                        None => spec.health,
                    };
                    match actor.health.hit(params, damage) {
                        Hit::Damaged => {
                            events.hits += 1;
                            actor.flash_left = FLASH_TICKS;
                        }
                        Hit::Killed => {
                            events.hits += 1;
                            events.kills += 1;
                            crate::actor_persistence::killed(actor.scene, actor.source_id);
                            if let Runtime::Blocker(b) = &mut actor.runtime {
                                b.controller.die();
                                crate::blocker_terrain::killed(actor.scene, actor.source_id);
                            }
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
            let shell = actor.acid_flyer_shell(spec);
            let tile = actor.mosquito_tile(spec);
            if !polygon.is_empty() && (hk_sim::polygon_hits_box(polygon, bounds)
                || tile.is_some_and(|t| hk_sim::polygon_hits_box(polygon, t))) {
                if deferred {other=resident(region.scene,x,y,&mut actor.located);}
                let (terrain_region,terrain_room)=other.as_ref()
                    .map_or((region,room),|(r,room)|(r,room));
                // NailSlash finds `BigBouncer` on the Acid Flyer's body and
                // calls BounceHigh for a down-slash instead of Bounce.
                if vitals.can_control() && !actor.buried() {
                    if shell.is_some() && nail.kind == 3 {
                        response.bounce_high(crate::NAIL_RESPONSE_PARAMS);
                    } else {
                        response.contact(
                            crate::NAIL_RESPONSE_PARAMS,
                            nail.kind,
                            player.facing,
                            player,
                        );
                    }
                }
                // `HealthManager::Hit` tests `IsBlockingByDirection` after the
                // evasion window and before `TakeDamage`, which is where
                // `ActorHealth::hit` already puts the invincible test. So the
                // Zombie Shield's live `SetInvincible` rides in as that flag,
                // with the cardinal `DirectionUtils::GetCardinalDirection`
                // would have produced: 0 right, 1 up, 2 left, 3 down. A raised
                // front shield leaves the down one open, which is the pogo.
                let cardinal = match nail.kind {
                    2 => 1u8,
                    3 => 3,
                    _ => if player.facing > 0 { 0 } else { 2 },
                };
                let health_params = match (actor.zombie_shield(), actor.blocker()) {
                    (Some(shield), _) => hk_sim::EnemyParams { invincible: shield.controller.blocks(cardinal), ..spec.health },
                    // The Blocker's shell is the same live `SetInvincible`, but
                    // it leaves `invincibleFromDirection` at the authored 0 and
                    // `IsBlockingByDirection` answers true for every cardinal in
                    // that case, so a shut Blocker blocks the pogo too.
                    (_, Some(blocker)) => hk_sim::EnemyParams { invincible: blocker.controller.invincible(), ..spec.health },
                    // `invincibleFromDirection` 7: up and down slashes bounce
                    // off; the body is never invincible otherwise.
                    _ if shell.is_some() => hk_sim::EnemyParams { invincible: hk_sim::acid_flyer::blocks(cardinal, false), ..spec.health },
                    _ if actor.buried() => hk_sim::EnemyParams { invincible: true, ..spec.health },
                    _ => spec.health,
                };
                match actor
                    .health
                    .hit(health_params, cheats.params(crate::VITAL_PARAMS).nail_damage)
                {
                    Hit::Damaged | Hit::Killed => {
                        events.hits += 1;
                        actor.flash_left = FLASH_TICKS;
                        if actor.health.dead {
                            events.kills += 1;
                            crate::actor_persistence::killed(actor.scene,actor.source_id);
                            if let Some(runner)=actor.runner_mut() {
                                runner.controller.die();
                                actor.emit_runner(RunnerEventKind::Destroy,&mut runner_event);
                            }
                            // The Shield's walk loop rides a Runner voice too.
                            if actor.zombie_shield().is_some() {
                                actor.emit_runner(RunnerEventKind::Destroy,&mut runner_event);
                            }
                            match &mut actor.runtime {
                                Runtime::Climber(c)=>c.controller.die(),
                                Runtime::Vengefly(f)=>f.controller.die(),
                                Runtime::Gruzzer(f)=>f.controller.die(),
                                Runtime::AcidFlyer(f)=>f.controller.die(),
                                Runtime::Mosquito(m)=>m.controller.die(),
                                Runtime::MossWalker(m)=>m.controller.die(),
                                Runtime::Baldur(r)=>r.controller.die(),
                                Runtime::Aspid(a)=>a.controller.die(),
                                Runtime::Hatcher(h)=>h.controller.die(),
                                // A baby is not removed: `Death` puts it back in
                                // the cage, which the next tick does through
                                // `park_baby` so the kill is counted first.
                                Runtime::HatcherBaby(_)=>{}
                                Runtime::ZombieShield(z)=>z.controller.die(),
                                Runtime::HuskGuard(g)=>g.controller.die(),
                                Runtime::Blocker(b)=>{b.controller.die();crate::blocker_terrain::killed(actor.scene,actor.source_id);}
                                Runtime::Pigeon(p)=>p.controller.die(),
                                // The False Knight answers the nail through its
                                // own branch above and never reaches this one.
                                Runtime::Walker|Runtime::Runner(_)|Runtime::Static {..}|Runtime::FalseKnight(_)|Runtime::Mawlek(_)
                                |Runtime::GruzMother(_)=>{}
                            }
                            state.enemy_death_particles(region,actor.source_id,[actor.x,actor.y,0]);
                            if let Some(corpse_spec)=spec.corpse {
                                let mut corpse=hk_sim::Corpse::spawn(corpse_spec,actor.x,actor.y,nail.kind,
                                    player.facing,actor.source_id);
                                // EmitCorpse chooses world X sign from the hit cardinal,
                                // cancelling the parent actor sign for horizontal strikes.
                                corpse.facing=if nail.kind<2 {-player.facing}else{actor.walk.direction};
                                // The Husk Guard's corpse is never flung: it
                                // stands where the guard stood, facing as it did.
                                if actor.husk_guard().is_some() {corpse.facing=actor.walk.direction;}
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
                            // TAKE DAMAGE wakes a Dormant Husk Guard.
                            if let Runtime::HuskGuard(guard) = &mut actor.runtime {
                                for action in guard.controller.took_damage().iter() {
                                    if let hk_sim::husk_guard::Action::Play(_) = action { guard.animation_tick = 0; }
                                }
                            }
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
                        // `HealthManager::TakeDamage` sends TOOK DAMAGE, which
                        // `Blocker Control` answers with a global transition
                        // into `Hit` from wherever it was. A blocked hit never
                        // reaches TakeDamage, so a shut Blocker is untouched.
                        if let Runtime::Blocker(blocker) = &mut actor.runtime {
                            if !actor.health.dead && !blocker.controller.took_damage().is_empty() {
                                blocker.animation_tick = 0;
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
                        // The Mosquito's `SetRecoilSpeed` zeroes it for a lunge.
                        let recoil_speed = actor.mosquito().map_or(spec.recoil_speed, |m| m.controller.recoil_speed());
                        actor.recoil = match nail.kind {
                            2 => [0, recoil_speed],
                            3 => [0, -recoil_speed],
                            _ => [player.facing * recoil_speed, 0],
                        };
                        }
                    }
                    // `HealthManager::Invincible` sends BLOCKED HIT, which only
                    // the four Shield states answer: the bump, then the chain.
                    Hit::Blocked => {
                        if let Runtime::ZombieShield(shield) = &mut actor.runtime {
                            let actions = shield.controller.blocked_hit();
                            if !actions.is_empty() { shield.animation_tick = 0; }
                        }
                        // `Invincible` sets evasionByHitRemaining 0.15 s.
                        if shell.is_some() { actor.health.evasion_ticks = hk_sim::acid_flyer::BLOCK_EVASION_TICKS; }
                    }
                    _ => {}
                }
            } else if let (false, Some(shell)) = (polygon.is_empty(), shell) {
                // The detached `Shell` holds no HealthManager: a slash there
                // only tinks (its recoils are NailSlash's own) or, from above,
                // takes an ordinary Bounce.
                if hk_sim::polygon_hits_box(polygon, shell) && vitals.can_control() {
                    response.contact(crate::NAIL_RESPONSE_PARAMS, nail.kind, player.facing, player);
                }
            }
            // The Acid Flyer's Shell carries its own DamageHero.
            let touched = (overlap(body, bounds) || shell.is_some_and(|b| overlap(body, b))) && !actor.buried();
            let mut damage = if touched { actor.health.contact_damage(spec.health) } else { 0 };
            // `Swipe`, armed for its clip: a second DamageHero reaching ahead.
            if let Some(guard) = actor.husk_guard() {
                if guard.swipe_left != 0 && overlap(body, guard.controller.swipe_box([actor.x, actor.y])) {
                    damage = hk_sim::husk_guard::SWIPE_DAMAGE;
                }
            }
            if damage != 0 {
                let direction = if player.x < actor.x { -1 } else { 1 };
                let hurt = cheats.hurt(vitals,crate::VITAL_PARAMS, damage, direction, false, player.shadow_dashing);
                if hurt != Hurt::Ignored {
                    events.hurt = hurt;
                }
            }
        }
        for (scene, position, velocity, kind, shot_clip, impact_clip) in fires.into_iter().flatten() {
            self.spawn_shot(scene, position, velocity, kind, shot_clip, impact_clip);
        }
        for (scene, origin, count, speed, angles, mut rng) in sprays.into_iter().flatten() {
            for _ in 0..count {
                let velocity = hk_sim::mawlek::shot_velocity(&mut rng, speed, angles);
                self.spawn_shot(scene, origin, velocity, ShotKind::Mawlek,
                    crate::mawlek_art::SHOT as u16, crate::mawlek_art::SHOT_IMPACT as u16);
            }
        }
        for (position, velocity) in releases.into_iter().flatten() {
            self.release_baby(region.scene, position, velocity);
        }
        if let Some(at) = reserve_release {
            self.release_reserve(region.scene, at);
        }
        if reserve_deaths != 0 {
            unsafe { HK_GZ_FLY_DEATHS = HK_GZ_FLY_DEATHS.saturating_add(reserve_deaths as u32) }
            for actor in self.actors.iter_mut().flatten().filter(|a| a.scene == region.scene) {
                if let Runtime::GruzMother(boss) = &mut actor.runtime {
                    for _ in 0..reserve_deaths { boss.arena.enemy_died(); }
                }
            }
        }
        for (scene, position, dir, spurt) in waved.into_iter().flatten() {
            self.spawn_wave(scene, position, dir, spurt);
        }
        // A pooled one-shot sprite rides the shot pool as a spent shot: it
        // plays its clip in place and hurts nothing. The sign of its vx is
        // only the draw's mirror (the Slam Effect art faces right).
        for (scene, position, facing, clip) in effects.into_iter().flatten() {
            self.spawn_projectile(Shot { scene: scene as u8, x: position[0], y: position[1], vx: -facing, vy: 0,
                animation_tick: 0, impact: true, kind: ShotKind::Spit, shot_clip: clip, impact_clip: clip });
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
        if self.waves.iter().any(Option::is_some) {
            let hurt = self.tick_waves(region, body, room.counts[5], &|i| state.edge(region, room, i),
                |damage, direction| cheats.hurt(vitals, crate::VITAL_PARAMS, damage, direction, false, shadow));
            if hurt != Hurt::Ignored { events.hurt = hurt; }
        }
        events
    }
    /// Create the immutable visible working set before preparing the GPU cache.
    pub fn prepare_draws(&self, region: &Region, room: &Room, camera: (i32, i32)) -> Draws {
        let mut draws = Draws {
            entries: [None; MAX_DRAWS],
            len: 0,
            keys: 0,
        };
        for (slot, actor) in self
            .actors
            .iter()
            .enumerate()
            .filter_map(|(slot, a)| Some((slot, a.as_ref()?)))
            .filter(|(_, a)| a.scene == region.scene)
        {
            let Some(spec) = self.spec_in(slot, region, actor.source_id) else {
                continue;
            };
            if let Some(boss)=actor.false_knight() {
                draw_false_knight(&mut draws,room,actor,&boss,camera);
                continue;
            }
            if let Some(boss)=actor.mawlek() {
                draw_mawlek(&mut draws,room,actor,&boss,camera);
                continue;
            }
            if let Some(boss)=actor.gruz() {
                draw_gruz(&mut draws,room,actor,&boss,camera);
                continue;
            }
            if actor.parked_reserve() {continue;}
            let (clip_id,animation_tick,position,facing)=if actor.health.dead {
                let (Some(corpse),Some(corpse_spec))=(actor.corpse,spec.corpse) else {continue;};
                if !corpse.visible(){continue;}
                (corpse.clip(corpse_spec),corpse.animation_tick,(corpse.x,corpse.y),corpse.facing)
            } else if let Some(runner)=actor.runner() {
                (runner.clip(spec),runner.animation_tick,(actor.x,actor.y),runner.controller.facing())
            } else if let Some(climber)=actor.climber() {
                (climber.clip(spec),climber.animation_tick,(actor.x,actor.y),actor.walk.direction)
            } else if let Some(fly)=actor.vengefly() {
                // The source sprite faces left (-1 native, +1 mirrored), as the Crawler.
                (fly.clip(spec),fly.animation_tick,(actor.x,actor.y),fly.controller.facing())
            } else if let Some(fly)=actor.gruzzer() {
                (spec.walk_clip,fly.animation_tick,(actor.x,actor.y),fly.controller.facing())
            } else if let Some(m)=actor.moss_walker() {
                (m.clip(spec),m.animation_tick,(actor.x,actor.y),m.controller.facing())
            } else if let Some(m)=actor.mosquito() {
                (m.clip(spec),m.animation_tick,(actor.x,actor.y),m.controller.facing())
            } else if let Some(fly)=actor.acid_flyer() {
                (fly.clip(spec),fly.animation_tick,(actor.x,actor.y),actor.walk.direction)
            } else if let Some(roller)=actor.baldur() {
                (roller.clip(spec),roller.animation_tick,(actor.x,actor.y),roller.controller.facing())
            } else if let Some(aspid)=actor.aspid() {
                (aspid.clip(spec),aspid.animation_tick,(actor.x,actor.y),aspid.controller.facing())
            } else if let Some(hatcher)=actor.hatcher() {
                (hatcher.clip(spec),hatcher.animation_tick,(actor.x,actor.y),hatcher.controller.facing())
            } else if let Some(baby)=actor.baby() {
                // A parked baby is inside the cage, which the source keeps off
                // the map. Nothing draws it there, and skipping it here is also
                // what keeps a full cage inside the per-frame animation budget.
                if actor.parked_baby() {continue;}
                (spec.walk_clip,baby.animation_tick,(actor.x,actor.y),baby.controller.facing())
            } else if let Some(shield)=actor.zombie_shield() {
                (shield.clip(spec),shield.animation_tick,(actor.x,actor.y),shield.controller.facing())
            } else if let Some(guard)=actor.husk_guard() {
                (guard.clip(spec),guard.animation_tick,(actor.x,actor.y),guard.controller.facing())
            } else if let Some(blocker)=actor.blocker() {
                // The Blocker never turns: the sprite keeps the mirror the cook
                // read off its transform for as long as it lives.
                (blocker.clip(spec),blocker.animation_tick,(actor.x,actor.y),actor.initial_direction as i32)
            } else if let Some(bird)=actor.pigeon() {
                // `Destroy` removes the GameObject, so a flown bird draws
                // nothing rather than hanging wherever its last tick left it.
                if bird.controller.phase()==hk_sim::pigeon::Phase::Gone {continue;}
                (bird.clip(spec),bird.animation_tick,(actor.x,actor.y),bird.controller.facing())
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
            // The corners below are placed as M * R(angle) * F * local, with M
            // the screen mirror (`position.0 - x`) and F the facing mirror. A
            // Climber faces -1 (F = M), so R(-angle) makes that R(angle): the
            // source's unmirrored rotation. Passing +angle drew the mirror image
            // of the source pose on every surface.
            let rotation = actor.climber().filter(|_| !actor.health.dead).map(|c| -c.controller.rotation())
                // The lunge's FaceAngle, by the same M * R * F reading: the
                // transform's z angle, negated (Q12 turns to Q16 degrees).
                .or_else(|| actor.mosquito().filter(|_| !actor.health.dead).map(|m| {
                    let [c, s] = m.controller.rotation();
                    -(psx_math::sincos::atan2_q12(s, c) as i32 * 5760)
                }))
                .unwrap_or(0);
            let turn = (rotation != 0).then(|| crate::world::debris::Rotation::new(rotation));
            let project = |b: [i32; 4]| {
                let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
                let mut verts = [(0i16, 0i16); 4];
                for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
                    let [x, y] = match turn { Some(t) => t.apply([x * facing, y]), None => [x * facing, y] };
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
                    draws.keys < MAX_VISIBLE && draws.len < MAX_DRAWS,
                    "visible enemy cache budget exceeded"
                );
                draws.push(texture as u16, verts, if actor.health.dead { 0 } else { actor.flash_left });
            }
        }
        for shot in self.shots.iter().flatten().filter(|s| s.scene as usize == region.scene) {
            if shot.kind == ShotKind::Mawlek {
                let clip = if shot.impact { shot.impact_clip } else { shot.shot_clip } as usize;
                if let Some(sprite) = crate::mawlek_art::BANK.frame(clip, shot.animation_tick) {
                    let facing = if shot.vx > 0 { 1 } else { -1 };
                    push_sprite_if_room(&mut draws, room, &crate::mawlek_art::BANK, sprite, (shot.x, shot.y), facing, camera);
                }
                continue;
            }
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
            if draws.keys >= MAX_VISIBLE || draws.len >= MAX_DRAWS { break; }
            draws.push(u32_at(f, 0) as u16, verts, 0);
        }
        // Every third spurt, which shows each of the clip's 3-tick frames once
        // along the trail rather than paying a quad for all eighteen.
        for (_, wave, spurt) in self.waves.iter().flatten().filter(|(id, _, _)| *id as usize == region.scene) {
            for (x, age) in wave.spurts(wave_params(*spurt), 3) {
                if *spurt == NO_SPURT {
                    let sprite = crate::fk_art::sprite(crate::fk_art::SPURT, age as u32);
                    push_sprite_if_room(&mut draws, room, &crate::fk_art::BANK, sprite, (x, wave.y), -(wave.dir as i32), camera);
                } else {
                    push_frame_if_room(&mut draws, room, clip_frame_index(room, *spurt, age as u32), (x, wave.y),
                        -(wave.dir as i32), camera);
                }
            }
        }
        draws
    }
}
/// A wave's cooked numbers: the False Knight's, or the Husk Guard's stomp.
const NO_SPURT: u16 = u16::MAX;
fn wave_params(spurt: u16) -> &'static hk_sim::shockwave::Params {
    if spurt == NO_SPURT { &crate::fk_art::WAVE } else { &hk_sim::husk_guard::WAVE }
}
/// The frame a room clip shows `tick` 60 Hz ticks in, played once.
fn clip_frame_index(room: &Room, clip: u16, tick: u32) -> usize {
    let clip = room.clip(clip as usize);
    let elapsed = (tick as u64 * clip[2] as u64 / (60 * 65536)) as u32;
    (clip[0] + elapsed.min(clip[1] - 1)) as usize
}
/// One actor-space box drawn at `position`, mirrored when `facing` is 1, the
/// way every actor frame here is.
fn project_box(b: [i32; 4], position: (i32, i32), facing: i32, camera: (i32, i32)) -> [(i16, i16); 4] {
    let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
    let mut verts = [(0i16, 0i16); 4];
    for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
        let x = position.0 - x * facing - camera.0;
        let y = position.1 + y - camera.1;
        *v = ((160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
              (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16);
    }
    verts
}
fn offscreen(verts: &[(i16, i16); 4]) -> bool {
    verts.iter().all(|v| v.0 < 0) || verts.iter().all(|v| v.0 >= 320)
        || verts.iter().all(|v| v.1 < 0) || verts.iter().all(|v| v.1 >= 240)
}
/// `project_box` for an object `depth` (Q16 units) behind the play plane:
/// everything it draws shrinks towards the camera's centre by the camera's
/// distance over its own.
fn project_box_at_depth(b: [i32; 4], position: (i32, i32), facing: i32, camera: (i32, i32), depth: i32) -> [(i16, i16); 4] {
    if depth == 0 { return project_box(b, position, facing, camera); }
    let k = |v: i32| ((v as i64 * CAMERA_DISTANCE as i64) / (CAMERA_DISTANCE as i64 + depth as i64)) as i32;
    let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
    let mut verts = [(0i16, 0i16); 4];
    for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
        let x = k(position.0 - x * facing - camera.0);
        let y = k(position.1 + y - camera.1);
        *v = ((160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
              (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16);
    }
    verts
}
/// Every part of one cooked boss sprite, culled part by part.
fn push_sprite(draws: &mut Draws, room: &Room, bank: &crate::boss_art::Bank, sprite: usize, position: (i32, i32),
               facing: i32, camera: (i32, i32), flash: u8, depth: i32) {
    for i in 0..bank.parts(sprite) {
        let (texture, b) = bank.part(room, sprite, i);
        let verts = project_box_at_depth(b, position, facing, camera, depth);
        if offscreen(&verts) { continue; }
        assert!(draws.len < MAX_DRAWS && (draws.keys < MAX_VISIBLE || !crate::render::streamed(texture as usize)),
            "boss draw budget exceeded");
        draws.push(texture, verts, flash);
    }
}
/// A sprite that yields to the budget instead of asserting: the wave's spurts
/// One room frame (every tile of it), yielding to the budget like the above:
/// the Husk Guard's spurts, which are ordinary actor-bank frames.
fn push_frame_if_room(draws: &mut Draws, room: &Room, frame: usize, position: (i32, i32), facing: i32, camera: (i32, i32)) {
    let (_, cols, rows) = room.frame_grid(frame);
    for tile in 0..cols * rows {
        let (texture, b) = room.frame_tile(frame, tile);
        let verts = project_box(b, position, facing, camera);
        if offscreen(&verts) { continue; }
        if draws.len >= MAX_DRAWS || draws.keys >= MAX_VISIBLE { return; }
        draws.push(texture as u16, verts, 0);
    }
}
/// A sprite that yields to the budget instead of asserting (see below).
fn push_sprite_if_room(draws: &mut Draws, room: &Room, bank: &crate::boss_art::Bank, sprite: usize,
                       position: (i32, i32), facing: i32, camera: (i32, i32)) {
    for i in 0..bank.parts(sprite) {
        let (texture, b) = bank.part(room, sprite, i);
        let verts = project_box(b, position, facing, camera);
        if offscreen(&verts) { continue; }
        let streamed = crate::render::streamed(texture as usize);
        if draws.len >= MAX_DRAWS || (streamed && draws.keys >= MAX_VISIBLE) { return; }
        draws.push(texture, verts, 0);
    }
}
/// The False Knight and everything that hangs off it: `Floor Control`'s floor
/// sprites behind, then the body, the `Head` in the open armour and the
/// `Death Head` once it has dropped out.
fn draw_false_knight(draws: &mut Draws, room: &Room, actor: &Actor, boss: &FalseKnightRuntime, camera: (i32, i32)) {
    let floor = crate::battle_gates::floor();
    let floor_art: &[u16] = match floor {
        crate::battle_gates::FLOOR_CRACKED => &crate::fk_art::FK_FLOOR_CRACKED,
        crate::battle_gates::FLOOR_BROKEN => &crate::fk_art::FK_FLOOR_BROKEN,
        _ => &[],
    };
    let [ax, ay] = crate::fk_art::FK_FLOOR_ANCHOR;
    for &sprite in floor_art {
        push_sprite(draws, room, &crate::fk_art::BANK, sprite as usize, (ax, ay), -1, camera, 0, 0);
    }
    // `Dormant` hangs the body inside the ceiling slab above the arena, where
    // the room's own scenery covers it in the source. Actor draws here are
    // not depth sorted against scenery, so the closer answer is to draw
    // nothing until it drops. A won arena never leaves `Dormant`.
    if boss.controller.phase() == hk_sim::false_knight::Phase::Dormant {
        return;
    }
    let facing = boss.facing();
    let position = (actor.x, actor.y);
    let bank = &crate::fk_art::BANK;
    push_sprite(draws, room, bank, boss.body_sprite(), position, facing, camera, actor.flash_left, 0);
    if let Some(sprite) = boss.head_sprite() {
        push_sprite(draws, room, bank, sprite, position, facing, camera, actor.flash_left, 0);
    }
    if let Some(sprite) = boss.death_head_sprite() {
        let [x, y] = boss.death_head_at(actor.x, actor.y);
        push_sprite(draws, room, bank, sprite, (x, y), facing, camera, 0, 0);
    }
}
/// Brooding Mawlek, back to front as the children's z orders them: the arms
/// (z +0.01), the `Spit Effect` (+0.002), the Head (+0.001), the body (0) and
/// the Dummy (-0.001) in front. The body never mirrors (its Walker's
/// `preventScaleChange`), so every part but the Dummy and `Mawlek Arm L` is
/// drawn as authored; the corpse replaces all of it once the body dies.
fn draw_mawlek(draws: &mut Draws, room: &Room, actor: &Actor, boss: &MawlekRuntime, camera: (i32, i32)) {
    use crate::mawlek_art as art;
    let bank = &art::BANK;
    let at = |offset: [i32; 2]| (actor.x + offset[0], actor.y + offset[1]);
    if actor.health.dead {
        if !boss.blown {
            let sprite = bank.sprite(art::CORPSE, boss.ticks[1] as u32);
            push_sprite(draws, room, bank, sprite, (actor.x, actor.y), -1, camera, 0, 0);
        }
        return;
    }
    let depth = boss.controller.depth();
    let flash = actor.flash_left;
    for arm in 0..2 {
        if let Some(sprite) = boss.sprite(2 + arm) {
            push_sprite(draws, room, bank, sprite, at(art::MW_ARM_OFFSET[arm]), if arm == 0 { -1 } else { 1 }, camera, flash, depth);
        }
    }
    if boss.spit != 0 {
        if let Some(sprite) = bank.frame(art::SPIT, boss.spit as u32 - 1) {
            push_sprite_if_room(draws, room, bank, sprite, at(art::MW_SPIT_OFFSET), -1, camera);
        }
    }
    if let Some(sprite) = boss.sprite(4) {
        push_sprite(draws, room, bank, sprite, at(art::MW_HEAD_OFFSET), -1, camera, flash, depth);
    }
    if boss.mesh {
        if let Some(sprite) = boss.sprite(0) {
            push_sprite(draws, room, bank, sprite, (actor.x, actor.y), -1, camera, flash, depth);
        }
    }
    if let Some(sprite) = boss.sprite(1) {
        let facing = -(boss.controller.dummy_scale() as i32);
        push_sprite(draws, room, bank, sprite, at(art::MW_DUMMY_OFFSET), facing, camera, flash, depth);
    }
}
/// Gruz Mother, its corpse or its burster: one sprite of its bank at the
/// actor's transform, mirrored by its scale sign (art authored facing left).
fn draw_gruz(draws: &mut Draws, room: &Room, actor: &Actor, boss: &GruzRuntime, camera: (i32, i32)) {
    if boss.controller.phase() == hk_sim::gruz_mother::Phase::Gone { return; }
    let bank = &crate::gruz_art::BANK;
    let Some(sprite) = bank.frame(boss.controller.clip() as usize, boss.tick as u32) else { return };
    let flash = if actor.health.dead { 0 } else { actor.flash_left };
    let sink = if boss.controller.sunk() { hk_sim::gruz_mother::SLAM_SINK } else { 0 };
    push_sprite(draws, room, bank, sprite, (actor.x, actor.y - sink), boss.facing(), camera, flash, 0);
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
#[derive(Clone, Copy)]
struct Draw {
    texture: u16,
    verts: [(i16, i16); 4],
    /// `flash_left` of the actor this frame belongs to; 0 draws no flash.
    flash: u8,
}
pub struct Draws {
    entries: [Option<Draw>; MAX_DRAWS],
    len: usize,
    /// Of `len`, the draws that stream through an animation slot.
    keys: usize,
}
impl Draws {
    fn push(&mut self, texture: u16, verts: [(i16, i16); 4], flash: u8) {
        self.entries[self.len] = Some(Draw { texture, verts, flash });
        self.len += 1;
        if crate::render::streamed(texture as usize) { self.keys += 1; }
    }
    pub fn append_needed(&self, needed: &mut [u16], len: &mut usize) {
        for entry in self.entries.iter().flatten() {
            // A texture in the scene's pages is already in VRAM.
            if !crate::render::streamed(entry.texture as usize) { continue; }
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
            if entry.flash != 0 {
                crate::render::texture_flash(entry.texture as usize, entry.verts, flash_tint(entry.flash));
            }
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
        if crate::battle_gates::armour_tink_hidden(target.bounds()) {
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

/// The spawning contract, which is a reservation rather than an allocation:
/// the cage is seated with the scene and a release only changes state.
#[cfg(test)]
mod hatcher_pool_tests {
    use super::*;
    const CAGE: [i32; 2] = [100 * ONE, 100 * ONE];
    /// Where the fixture's placements stand; only the Hatcher wakes alert.
    const AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
        source_id: 0, x: 0, y: 0, initial_direction: -1, random_start_direction: false,
        start_alert: false, start_right: false, rotation_q16: 0,
    };
    const HATCHER_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
        source_id: 5007, x: 20 * ONE, y: 20 * ONE, start_alert: true, ..AT };
    const BABY_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
        source_id: 5010, x: CAGE[0], y: CAGE[1], ..AT };
    const BABY_1_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement { source_id: 5011, ..BABY_AT };
    const BABY_2_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement { source_id: 5012, ..BABY_AT };
    const HATCHER: ActorSpec = ActorSpec {
        controller: ActorController::Hatcher { fire_clip: 1 },
        bounds: [-51200, -71680, 34816, 49152],
        health: hk_sim::EnemyParams { health: 20, contact_damage: 1, evasion_ticks: 12,
            invincible: false, damage_override: false },
        walk: hk_sim::WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
        walk_clip: 0, turn_clip: 0,
        corpse: None, recoil_speed: 20 * ONE, recoil_ticks: 9, dream_soul: 33,
    };
    const BABY: ActorSpec = ActorSpec {
        controller: ActorController::HatcherBaby,
        bounds: [-13312, -14336, 10240, 10240],
        health: hk_sim::EnemyParams { health: 5, contact_damage: 1, evasion_ticks: 12,
            invincible: false, damage_override: false },
        walk: hk_sim::WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
        walk_clip: 0, turn_clip: 0,
        corpse: None, recoil_speed: 0, recoil_ticks: 0, dream_soul: 33,
    };
    // Three cage members of one type: one spec, three placements.
    static SPECS: [(hk_sim::ActorPlacement, &ActorSpec); 4] = [(HATCHER_AT, &HATCHER),
        (BABY_AT, &BABY), (BABY_1_AT, &BABY), (BABY_2_AT, &BABY)];
    /// Two clips over two textures: Fly (six frames, 12 fps, loop from 2) and
    /// Fire (one frame, once), which is all the draw path reads.
    fn bytes() -> Vec<u8> {
        let lengths = [6u32, 1];
        let mut b = Vec::from(*b"HKROOM02");
        for n in [1u32, 2, 0, 7, 2, 1, 0, 0] { b.extend(n.to_le_bytes()); }
        for _ in 0..2 { for n in [0u16, 0, 0, 4, 4, 0] { b.extend(n.to_le_bytes()); }
            b.extend(0u32.to_le_bytes()); }
        for (clip, len) in lengths.iter().enumerate() {
            for _ in 0..*len { for n in [clip as i32, -ONE / 2, -ONE, ONE / 2, 0] { b.extend(n.to_le_bytes()); } }
        }
        let mut first = 0;
        for (i, len) in lengths.iter().enumerate() {
            let wrap = if i == 0 { (2 << 16) | 1 } else { 2 };
            for n in [first, *len, 12 * 65536, wrap] { b.extend(n.to_le_bytes()); }
            first += len;
        }
        for n in [0, 0, 0, 0] { b.extend((n as i32).to_le_bytes()); }
        b.resize(b.len() + 2 * 32 + 32768, 0);
        b
    }
    fn region() -> Region {
        Region { scene: 0, bounds: [-100 * ONE, -10 * ONE, 100 * ONE, 60 * ONE],
            collision_bounds: [-100 * ONE, -10 * ONE, 100 * ONE, 60 * ONE], actors: &SPECS }
    }
    fn context(cage_children: u16) -> RunnerContext {
        let hero = [20 * ONE, 18 * ONE];
        RunnerContext { camera: [0, 0, -2496922], hero,
            hero_body: [hero[0] - ONE / 2, hero[1] - ONE, hero[0] + ONE / 2, hero[1] + ONE],
            cage_children }
    }
    fn world() -> EnemyWorld {
        let mut world = EnemyWorld::new();
        world.sync_region(&region());
        world
    }
    fn parked(world: &EnemyWorld) -> usize {
        world.actors.iter().flatten().filter(|a| a.parked_baby()).count()
    }
    /// Drive the Hatcher until it asks for a release, then answer it the way
    /// `tick` does. Returns the release position and where the Hatcher was.
    fn fire(world: &mut EnemyWorld, room: &Room) -> ([i32; 2], [i32; 2]) {
        for _ in 0..600 {
            let mut asked = None;
            let free = parked(world) as u16;
            let actor = world.actors.iter_mut().flatten()
                .find(|a| a.source_id == HATCHER_AT.source_id).expect("the Hatcher is seated");
            actor.advance(&HATCHER, room, 1, |_| [0; 4], context(free), &mut |event| {
                if let RunnerEventKind::Release { velocity } = event.kind {
                    asked = Some((event.position, velocity));
                }
            });
            let at = [actor.x, actor.y];
            if let Some((position, velocity)) = asked {
                world.release_baby(0, position, velocity);
                return (position, at);
            }
        }
        panic!("the Hatcher never released with a full cage");
    }
    #[test]
    fn the_cage_is_seated_with_the_scene_and_starts_parked() {
        let world = world();
        assert_eq!(world.actors.iter().flatten().count(), 4);
        assert_eq!(parked(&world), 3);
        // A parked member sits where the cooker found the cage, off the map.
        for actor in world.actors.iter().flatten().filter(|a| a.parked_baby()) {
            assert_eq!((actor.x, actor.y), (CAGE[0], CAGE[1]));
            assert_eq!(actor.health.hp, 5);
        }
    }
    #[test]
    fn a_release_wakes_one_reserved_member_below_the_hatcher() {
        let b = bytes();
        let room = Room::parse(&b).unwrap();
        let mut world = world();
        let (position, at) = fire(&mut world, &room);
        // `Fire` reads the Hatcher's own transform, which Distance Fly has
        // been moving, and drops the baby one unit under it.
        assert_ne!(at, [HATCHER_AT.x, HATCHER_AT.y]);
        assert_eq!(position, [at[0], at[1] - ONE]);
        assert_eq!(parked(&world), 2);
        let live: Vec<_> = world.actors.iter().flatten()
            .filter(|a| a.baby().is_some() && !a.parked_baby()).collect();
        assert_eq!(live.len(), 1);
        assert_eq!((live[0].x, live[0].y), (position[0], position[1]));
        assert_eq!(live[0].vy, -5 * ONE);
        // No slot was created: the pool is the same four actors it started as.
        assert_eq!(world.actors.iter().flatten().count(), 4);
    }
    #[test]
    fn an_empty_cage_releases_nothing_and_creates_nothing() {
        let b = bytes();
        let room = Room::parse(&b).unwrap();
        let mut world = world();
        for _ in 0..3 { fire(&mut world, &room); }
        assert_eq!(parked(&world), 0);
        // With the cage empty the controller never asks, so 600 frames of
        // Distance Fly pass without a release and the pool does not move.
        for _ in 0..600 {
            let actor = world.actors.iter_mut().flatten()
                .find(|a| a.source_id == HATCHER_AT.source_id).expect("the Hatcher is seated");
            actor.advance(&HATCHER, &room, 1, |_| [0; 4], context(0), &mut |event| {
                assert!(!matches!(event.kind, RunnerEventKind::Release { .. }),
                    "released from an empty cage");
            });
        }
        assert_eq!(world.actors.iter().flatten().count(), 4);
        // And a release the runtime is asked for anyway finds nothing to wake.
        world.release_baby(0, [0, 0], [0, -5 * ONE]);
        assert_eq!(parked(&world), 0);
        assert_eq!(world.actors.iter().flatten().filter(|a| a.baby().is_some()).count(), 3);
    }
    #[test]
    fn death_returns_a_baby_to_the_cage_rather_than_removing_it() {
        let b = bytes();
        let room = Room::parse(&b).unwrap();
        let mut world = world();
        fire(&mut world, &room);
        let actor = world.actors.iter_mut().flatten()
            .find(|a| a.baby().is_some() && !a.parked_baby()).expect("one baby is out");
        assert_eq!(actor.health.hit(BABY.health, 5), Hit::Killed);
        assert!(actor.health.dead);
        actor.park_baby(&BABY_AT, &BABY);
        assert!(!actor.health.dead);
        assert_eq!(actor.health.hp, 5);
        assert_eq!((actor.x, actor.y, actor.vy), (CAGE[0], CAGE[1], 0));
        assert_eq!(parked(&world), 3);
    }
    #[test]
    fn a_parked_member_never_draws_and_a_released_one_does() {
        let b = bytes();
        let room = Room::parse(&b).unwrap();
        let mut world = world();
        let camera = (HATCHER_AT.x, HATCHER_AT.y);
        // Only the Hatcher is anywhere the camera can see it.
        assert_eq!(world.prepare_draws(&region(), &room, camera).len, 1);
        fire(&mut world, &room);
        assert_eq!(world.prepare_draws(&region(), &room, camera).len, 2);
    }
}

#[cfg(test)]
mod runner_runtime_tests {
    use super::*;
    use hk_sim::runner::{Swipe, Walker};
    pub(super) const SPEC: ActorSpec = ActorSpec {
        controller: ActorController::Runner {
            idle_clip: 2, anticipate_clip: 3, lunge_clip: 4, cooldown_clip: 5,
            params: hk_sim::runner::Params::RUNNER, alert: hk_sim::runner_senses::ALERT_LOCAL,
        },
        bounds: [-51200,-94208,28672,12288],
        health: hk_sim::EnemyParams {health:15,contact_damage:1,evasion_ticks:12,
            invincible:false,damage_override:false},
        walk:hk_sim::WalkParams {speed:ONE+ONE/2,turn_ticks:10,turn_cooldown_ticks:60},
        walk_clip:0,turn_clip:1,
        corpse:Some(hk_sim::CorpseSpec {air_clip:2,land_clip:2,bounds:[-ONE/2,-ONE,ONE/2,0],
            spawn_offset:[0,ONE/2],bounce_factor:13107, fling_speed: 15 * hk_sim::ONE, gravity: 48 * hk_sim::ONE, breaker: false, smash_bounces: 0, remove_after_land: 0, hold_ticks: 0}),
        recoil_speed:10*ONE,recoil_ticks:9, dream_soul: 0
    };
    pub(super) const AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
        source_id: 5196, x: 0, y: 94208, initial_direction: -1, random_start_direction: false,
        start_alert: false, start_right: false, rotation_q16: 0,
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
        let y=AT.y;
        RunnerContext {camera:[0,2*ONE,-2496922],hero:[hero_x,y],
            hero_body:[hero_x-crate::PARAMS.half_width,y+crate::PARAMS.bottom,
                hero_x+crate::PARAMS.half_width,y+crate::PARAMS.top],cage_children:0}
    }
    fn advance(actor:&mut Actor,room:&Room,c:RunnerContext,events:&mut Vec<RunnerEvent>) {
        actor.advance(&SPEC,room,room.counts[5],|i|room.edge(i),c,&mut |e|events.push(e));
    }
    fn phase(actor:&Actor)->Swipe {actor.runner().unwrap().controller.swipe()}
    fn region()->Region {
        Region {scene:0,bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],
            collision_bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],actors:&[(AT,&SPEC)]}
    }
    #[test]
    fn actual_runner_clock_motion_clips_and_ordered_effects() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut actor=Actor::new(0,&AT,&SPEC);let mut events=Vec::new();let c=context(3*ONE);
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
        let mut actor=Actor::new(0,&AT,&SPEC);let mut events=Vec::new();
        let mut c=context(30*ONE);c.camera=[50*ONE,AT.y,-40*ONE];
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Waiting);
        assert_eq!(actor.x,AT.x);assert!(events.is_empty());
        c.camera=[0,AT.y,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        let before=actor.x;
        for _ in 0..40 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        assert!(actor.x<before);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        c.camera=[200*ONE,200*ONE,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner().unwrap().controller.walker(),Walker::Walking);
        let actor=Actor::new(0,&AT,&SPEC);let runner=actor.runner().unwrap();
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
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,AT.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO };
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();let mut state=State;let mut response=hk_sim::NailResponse::new();
        let cheats=crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()};
        // Coincident origin has conservatively unknown LOS. Accepted damage must
        // nevertheless enter anticipation BEFORE its horizontal reset restarts Walk.
        world.sync_region(&r);nail.active=true;nail.age=1;
        let hit=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
            attack,[poly;4],cheats,[0,AT.y,-2496922],|e|events.push(e),|_,_,_,_|None);
        assert_eq!((hit.hits,hit.kills),(1,0));
        assert_eq!(world.actors[0].unwrap().health.hp,10);
        let chase=events.iter().position(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})).unwrap();
        assert!(events[chase+1..].iter().any(|e|e.kind==RunnerEventKind::AudioPlay));
        assert_eq!(world.actors[0].unwrap().recoil_left,9);
        let after_first=events.len();
        world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
            attack,[poly;4],cheats,[0,AT.y,-2496922],|e|events.push(e),|_,_,_,_|None);
        assert_eq!(world.actors[0].unwrap().health.hp,10); // evaded overlap
        assert!(!events[after_first..].iter().any(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})));
        // Isolate two further accepted health events from traversal/recoil timing.
        for expected in [5,0] {
            let actor=world.actors[0].as_mut().unwrap();
            actor.health.evasion_ticks=0;actor.recoil_left=0;player.x=actor.x;player.y=actor.y;
            let result=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,&mut response,
                attack,[poly;4],cheats,[0,AT.y,-2496922],|e|events.push(e),|_,_,_,_|None);
            assert_eq!(world.actors[0].unwrap().health.hp,expected);
            assert_eq!(result.kills,u16::from(expected==0));
        }
        let actor=world.actors[0].unwrap();assert!(actor.corpse.is_some());
        assert_eq!(phase(&actor),Swipe::Dead);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::Destroy).count(),1);
        let mut paid=Vec::new();
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        assert_eq!(paid,[AT.source_id]);
        assert_eq!(vitals.soul,33);
    }
    #[test]
    fn source_seed_and_effect_pitch_sequence_are_deterministic() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut a=Actor::new(0,&AT,&SPEC);let mut b=Actor::new(0,&AT,&SPEC);
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
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,AT.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        nail.active=true;nail.age=1;nail.kind=2;
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO };
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();
        let hit=world.tick(&r,&room,&mut State,&mut player,&mut vitals,&nail,&hk_sim::DreamNail::new(),None,
            &mut hk_sim::NailResponse::new(),attack,[poly;4],
            crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()},
            [0,AT.y,-2496922],|e|events.push(e),|_,_,_,_|None);
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
            world.tick(r,&room,&mut State,&mut Player::spawn(30*ONE,AT.y),
                &mut Vitals::new(crate::VITAL_PARAMS),&Nail::new(),&hk_sim::DreamNail::new(),None,
                &mut hk_sim::NailResponse::new(),
                AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8, ..AttackParams::ZERO },
                [&[];4],crate::cheats::Settings::new(),[0,AT.y,-2496922],
                |e|events.push(e),|_,_,_,_|None);
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

#[cfg(test)]
mod blocker_shot_tests {
    use super::*;
    /// `Shot Mawlek` and `Spitter Shot R` are the same box on the same pool and
    /// differ only in gravity, which is exactly the kind of thing a shared pool
    /// gets wrong quietly. The Blocker lobs its goop over a gap at 20 up; the
    /// Aspid's bullet at the same speed would still be climbing long after.
    #[test]
    fn blocker_goop_arcs_over_where_an_aspid_shot_would_not() {
        let mut world = EnemyWorld::new();
        world.spawn_shot(0, [0, 0], [0, 20 * ONE], ShotKind::Goop, 0, 0);
        world.spawn_shot(0, [0, 0], [0, 20 * ONE], ShotKind::Spit, 0, 0);
        // `tick_shots` needs a region and terrain; a scene with no edges and
        // wide bounds leaves gravity as the only thing acting on either.
        let region = crate::world::Region { scene: 0,
            bounds: [-400 * ONE, -400 * ONE, 400 * ONE, 400 * ONE],
            collision_bounds: [-400 * ONE, -400 * ONE, 400 * ONE, 400 * ONE], actors: &[] };
        let mut peaks = [None; 2];
        for t in 1..=60u32 {
            world.tick_shots(&region, [i32::MIN / 2; 4], 0, &|_| [0; 4], |_, _| Hurt::Ignored);
            for (slot, peak) in world.shots.iter().zip(peaks.iter_mut()) {
                let shot = slot.expect("neither shot ends without terrain or a hero");
                if shot.vy <= 0 && peak.is_none() { *peak = Some(t); }
            }
        }
        // 20 / (.6 * 60) seconds in ticks, against 20 / (.05 * 60), which is
        // more than six seconds and so has not turned over inside this run.
        assert_eq!(peaks[0], Some(34), "goop apex");
        assert_eq!(peaks[1], None, "an Aspid shot is still climbing after a second");
    }
}
/// `sync_region`'s placement table against the search it replaces: random
/// placement lists over two scenes, with source ids shared between lists and
/// repeated within one, seated, swapped and reset in random order.
#[cfg(test)]
mod placement_table_tests {
    use super::*;
    static SPECS: [ActorSpec; 2] = [runner_runtime_tests::SPEC, ActorSpec { walk_clip: 7, ..runner_runtime_tests::SPEC }];
    #[test]
    fn placement_table_answers_the_placement_search() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |n: u64| { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed % n) as usize };
        let mut regions = Vec::new();
        for _ in 0..24 {
            let scene = next(2);
            let count = next(7);
            let actors: Vec<_> = (0..count).map(|_| {
                let placement = hk_sim::ActorPlacement { source_id: next(10) as u32, x: next(64) as i32 * ONE,
                    ..runner_runtime_tests::AT };
                (placement, &SPECS[next(2)])
            }).collect();
            regions.push(crate::world::Region { scene, bounds: [0; 4], collision_bounds: [0; 4],
                actors: Box::leak(actors.into_boxed_slice()) });
        }
        let mut world = EnemyWorld::new();
        let mut checked = 0;
        for _ in 0..4000 {
            if next(10) == 0 { world.reset_scene(next(2)); }
            let region = regions[next(regions.len() as u64)];
            world.sync_region(&region);
            // Seated in full, as an unconditional sync would leave it.
            for (placement, _) in crate::world::region_actors(&region) {
                assert!(world.actors.iter().flatten().any(|a| a.scene == region.scene && a.source_id == placement.source_id));
            }
            let other = regions[next(regions.len() as u64)];
            for (slot, actor) in world.actors.iter().enumerate() {
                let Some(actor) = actor else { continue };
                for r in [region, other] {
                    if actor.scene != r.scene { continue; }
                    let found = crate::world::region_actors(&r).find(|(p, _)| p.source_id == actor.source_id);
                    let spec = world.spec_in(slot, &r, actor.source_id);
                    assert_eq!(spec.map(|s| s as *const _), found.map(|(_, s)| s as *const _));
                    if core::ptr::eq(r.actors, region.actors) && r.scene == region.scene {
                        assert_eq!(world.placed_spec[slot].map(|s| s as *const _), found.map(|(_, s)| s as *const _));
                        if let Some((placement, _)) = found {
                            let (listed, _) = crate::world::region_actor(&r, world.placement[slot] as usize).unwrap();
                            assert_eq!(listed, placement);
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 1000, "{checked}");
    }
}
