//! Greenpath's Hornet as the actor runtime runs it: the boss tree's FSMs (hornet.rs) plus everything the original does natively
//! around them. Written from what the original does at the boundary, not from its code:
//!
//! - the **body**: `Hornet Boss 1`'s Rigidbody2D against the terrain, held in its arena by `ConstrainPosition`, and the
//!   bodies of the objects it throws;
//! - the **senses**: which of the tree's trigger volumes the Knight's body overlaps this step (`Ent::trig`), which sides of the
//!   body touched something (`CheckCollisionSide`), the sprite animators' completion and frame triggers;
//! - the **HealthManager and Recoil**: nail strikes against the boss's collider, `TOOK DAMAGE`, the recoil, `SetInvincible`;
//! - the **DamageHero** boxes of every active hurt object;
//! - the **death**: the corpse prefab's own FSM runs (it writes `hornet1Defeated` and ends by sending `HORNET LEAVE`);
//! - everything an action asked the engine for, as `Effect`s the caller carries out.
//!
//! The caller owns the sprite art, the sounds, the particles and the camera. See docs/FSM-V2-DESIGN.md 2.4.
use crate::fsm3_world::{flag, k, Ent};
use hk_format::fsm3::NONE8;
use crate::hornet::{corpse, fight, Boss};
use crate::{ActorHealth, EnemyParams, Hit, ONE};

/// Where the fight is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not started: the tree runs (its Encounter waits for the Knight) and the boss cannot be hurt yet.
    Asleep,
    Fighting,
    /// Dead: the corpse tree runs.
    Dying,
    /// Over: nothing runs.
    Gone,
}

/// What an action asked of the engine. Object handles are the fight tree's; `corpse` effects come from the corpse tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// A sound: the cooker's clip id, volume and pitch (Q16.16). `looped` is a voice the state owns.
    Audio { object: u8, clip: i32, volume: i32, pitch: i32, looped: bool },
    AudioStop { object: u8 },
    Music { cue: i32, delay: i32, transition: i32 },
    Snapshot { id: i32 },
    /// A particle emitter on an object: a burst of `emit` (emit > 0), or stopped.
    Particles { object: u8, on: bool, emit: i32 },
    /// A message to something outside the tree (the camera shaker, the gates, the dialogue box): the event id, the delay in
    /// ticks and the delivery mode.
    Message { object: u8, event: i32, ticks: i32, mode: i32 },
    /// A native component call: behaviour and method (cooked string ids) and the first argument.
    Call { object: u8, behaviour: i32, method: i32, arg: i32 },
    /// `DialogueBox.StartConversation`: the cooked id of the conversation's key.
    Conversation { id: i32 },
    /// A pooled object spawned at a place; `handle` is the tree handle the adapter gave it.
    Spawn { pool: i32, at: [i32; 2], handle: u8 },
    /// Spawned pieces flung from an object (`SpawnRandomObjectsV2`).
    Fling { prefab: i32, count: i32, speed: i32 },
    Destroy { object: u8 },
    /// `HeroController.RelinquishControl` / `RegainControl`.
    HeroControl { regain: bool },
    /// A PlayerData field of the tree's reserve changed (index into `HORNET_FIELDS`).
    Persist { field: u8, value: i32 },
    /// The Knight is hurt by an active DamageHero object, by this much, away from `from_x`.
    HeroHurt { damage: u8, from_x: i32 },
    /// The boss was hit and took damage; `hp` is what is left.
    Struck { hp: i32 },
    /// The corpse sent `HORNET LEAVE`: the Cloak Corpse starts to fall.
    CloakLeaves,
    /// The arena's battle gates close (`BG CLOSE`, from the Encounter) or open (`HORNET KILLED`).
    GatesClose,
    GatesOpen,
    /// `EnemyKillShake` (average: false) or `AverageShake`.
    Shake { average: bool },
    /// The boss died: the corpse tree has been spawned.
    Died,
}

/// Index into the tree's PlayerData reserve, as cooked (`boss::HORNET_FIELDS`).
pub mod field {
    pub const HORNET_GREENPATH: u8 = 0;
    pub const HORNET1_DEFEATED: u8 = 1;
    pub const DISABLE_PAUSE: u8 = 2;
    pub const DREAMER_SCENE1: u8 = 3;
    pub const HAS_DASH: u8 = 4;
    pub const EQUIPPED_CHARM_15: u8 = 5;
}

/// A nail strike this tick: the slash polygon and the direction (0 horizontal, 2 up, 3 down), with the damage it deals.
#[derive(Clone, Copy)]
pub struct Strike<'a> {
    pub polygon: &'a [[i32; 2]],
    pub kind: u8,
    pub damage: i32,
    pub facing: i32,
}

/// What the world gave the fight this tick.
pub struct Input<'a> {
    /// The Knight's feet, his hurt box (min x, min y, max x, max y) and the camera.
    pub hero: [i32; 2],
    pub hero_body: [i32; 4],
    pub camera: [i32; 3],
    /// Elapsed game and wall time, in Q16.16 ticks (65536 = one 60 Hz tick).
    pub dt_scaled: i32,
    pub dt_real: i32,
    pub strike: Option<Strike<'a>>,
    /// The conversation the last `Effect::Conversation` started has finished.
    pub conversation_done: bool,
}

/// The fight in progress. Large (two trees with their object tables): the actor runtime keeps one, in the room module's data.
pub struct Fight {
    pub boss: Boss,
    pub phase: Phase,
    pub health: ActorHealth,
    params: EnemyParams,
    /// FSM-visible PlayerData as of the last step, to find what a pass wrote.
    pd_seen: [i32; 8],
    /// Bit per object: the Knight was inside its trigger last step.
    inside: u32,
    /// Contact sides of the body last step (bit 0 up, 1 right, 2 down, 3 left).
    contacts: u8,
    corpse_contacts: u8,
    /// Fixed-step accumulator: Unity's 50 Hz against this 60 Hz tick.
    fixed: u32,
    recoil_left: u16,
    recoil_vx: i32,
    recoil_speed: i32,
    invincible: bool,
    /// Simulation ticks since `begin`.
    pub tick: u32,
    /// This tick's strike reached her collider (landed, blocked or inside her evasion window).
    pub reached: bool,
}

const PD_FIELDS: usize = 8;
/// The bytes of the room's state block (host/code_modules.py `art_hornet`): a Fight lives there, in zeroed pool memory.
pub const STATE_BYTES: usize = 6144;

impl Fight {
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub const fn blank() -> Self {
        Self {
            boss: Boss::blank(),
            phase: Phase::Gone,
            health: ActorHealth::new(EnemyParams { health: 0, contact_damage: 0, evasion_ticks: 0, invincible: false, damage_override: false }),
            params: EnemyParams { health: 0, contact_damage: 0, evasion_ticks: 0, invincible: false, damage_override: false },
            pd_seen: [0; PD_FIELDS],
            inside: 0,
            contacts: 0,
            corpse_contacts: 0,
            fixed: 0,
            recoil_left: 0,
            recoil_vx: 0,
            recoil_speed: 0,
            invincible: false,
            tick: 0,
            reached: false,
        }
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Lay a fight out. `defeated` is the saved `hornet1Defeated`: Hornet is gone for good (`DeactivateIfPlayerdataTrue`), and
    /// nothing runs.
    pub fn begin(&mut self, seed: u32, defeated: bool, pd: [i32; PD_FIELDS]) {
        // SAFETY: every field is plain data for which all-zero bytes are a valid value (null pointers, `Phase::Asleep`, empty
        // tables). Zeroing in place spares the stack a Fight-sized temporary.
        unsafe { core::ptr::write_bytes(self as *mut Self as *mut u8, 0, core::mem::size_of::<Self>()) };
        self.boss.bind(&fight::DATA);
        self.boss.env = crate::fsm3::Env::new();
        self.phase = Phase::Gone;
        if defeated {
            return;
        }
        self.params = EnemyParams {
            health: fight::HEALTH as i16,
            contact_damage: 0,
            evasion_ticks: psx_math::int32::mul_shr_i32(fight::INVULNERABLE_TIME, 60, 16) as u16,
            invincible: false,
            damage_override: false,
        };
        self.health = ActorHealth::new(self.params);
        self.recoil_speed = fight::RECOIL_SPEED;
        self.boss.pd = pd;
        self.boss.init(seed);
        self.pd_seen = pd;
        self.phase = Phase::Asleep;
        // The first pass enters every active FSM's start state.
        self.boss.enter();
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// One 60 Hz tick.
    pub fn step(&mut self, input: &Input<'_>, count: usize, edge: &dyn Fn(usize) -> [i32; 4], sink: &mut impl FnMut(Effect)) {
        self.tick = self.tick.wrapping_add(1);
        self.reached = false;
        match self.phase {
            Phase::Gone => {}
            Phase::Dying => self.step_corpse(input, count, edge, sink),
            Phase::Asleep | Phase::Fighting => self.step_fight(input, count, edge, sink),
        }
    }

    // ---- the fight --------------------------------------------------------------------------------------------------

    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn step_fight(&mut self, input: &Input<'_>, count: usize, edge: &dyn Fn(usize) -> [i32; 4], sink: &mut impl FnMut(Effect)) {
        self.health.tick_flash();
        self.health.tick();
        let b = &mut self.boss;
        b.env.hero = [input.hero[0], input.hero[1], 0];
        b.env.camera = input.camera;
        b.env.dt_scaled = input.dt_scaled;
        b.env.dt_real = input.dt_real;
        // Unity's fixed step is 50 Hz: three of these in every five ticks owe two.
        self.fixed += 50;
        let mut steps = 0;
        while self.fixed >= 60 {
            self.fixed -= 60;
            steps += 1;
        }
        let hero_body = input.hero_body;
        self.update_triggers(hero_body);
        self.count_animations();
        if let Some(s) = input.strike {
            self.strike(&s, sink);
        }
        if input.conversation_done {
            self.boss.tree().event(fight::FSM_ENCOUNTER, fight::EV_CONVO_FINISH);
        }
        // Physics (the body), then the contacts it reports, then the scripts.
        let contacts = self.move_body(count, edge);
        self.report_contacts(contacts);
        self.boss.step(steps);
        self.constrain();
        self.drain(sink, input);
        self.check_pd(sink);
        self.hurt_hero(hero_body, sink);
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// The Knight against every trigger volume of the tree: entered, inside, left, as Unity's callbacks give them.
    fn update_triggers(&mut self, hero_body: [i32; 4]) {
        let w = &mut self.boss.world;
        for i in 0..w.n {
            let live = {
                let e = &w.ent[i as usize];
                e.on(flag::TRIGGER) && e.on(flag::COLLIDER) && w.active_in_hierarchy(i)
            };
            let now = live && overlap(world_box(w, i), hero_body);
            let was = self.inside & (1 << i) != 0;
            let mut t = 0;
            if now && !was {
                t |= 1;
            }
            if now {
                t |= 2;
            }
            if was && !now {
                t |= 4;
            }
            w.ent[i as usize].trig = t;
            if now {
                self.inside |= 1 << i;
            } else {
                self.inside &= !(1 << i);
            }
        }
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Sprite animators: `anim_left` runs down, and a clip's frame triggers raise `anim_event` as their frames show.
    fn count_animations(&mut self) {
        animate(&mut self.boss.world);
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// The boss's Rigidbody2D against the terrain; returns the contact sides (bit 0 up, 1 right, 2 down, 3 left).
    fn move_body(&mut self, count: usize, edge: &dyn Fn(usize) -> [i32; 4]) -> u8 {
        // A recoil sweeps a separate displacement over its few ticks; it is the Recoil component's, not the FSM's.
        let recoil = if self.recoil_left != 0 {
            self.recoil_left -= 1;
            Some(self.recoil_vx)
        } else {
            None
        };
        body_step(&mut self.boss.world, self.contacts, recoil, count, edge)
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// `CheckCollisionSide(Enter)` is told about sides that just touched, `CheckCollisionSide` about all that touch.
    fn report_contacts(&mut self, now: u8) {
        let entered = now & !self.contacts;
        self.contacts = now;
        if entered != 0 {
            self.boss.env.contacts = entered;
            self.boss.tree().step(crate::fsm3::PH_COLLISION_ENTER);
        }
        if now != 0 {
            self.boss.env.contacts = now;
            self.boss.tree().step(crate::fsm3::PH_COLLISION_STAY);
        }
        self.boss.env.contacts = 0;
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// `ConstrainPosition`: the body stays between the arena's walls.
    fn constrain(&mut self) {
        let e = &mut self.boss.world.ent[0];
        let [lo, hi] = fight::ARENA_X;
        e.pos[0] = e.pos[0].clamp(lo, hi);
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// A nail strike against the boss's collider (the HealthManager) and the Encounter's box (`ATTACKED`).
    fn strike(&mut self, s: &Strike<'_>, sink: &mut impl FnMut(Effect)) {
        // The Encounter starts on a strike as on the Knight's walking in.
        let mut struck: u32 = 0;
        {
            let w = &self.boss.world;
            for i in 1..w.n {
                let e = &w.ent[i as usize];
                if e.on(flag::COLLIDER) && e.on(flag::TRIGGER) && w.active_in_hierarchy(i) && crate::polygon_hits_box(s.polygon, world_box(w, i)) {
                    struck |= 1 << i;
                }
            }
        }
        for (f, &o) in fight::OWNERS.iter().enumerate() {
            if struck & (1 << o) != 0 && f as u8 == fight::FSM_ENCOUNTER {
                self.boss.tree().event(f as u8, fight::EV_ATTACKED);
            }
        }
        let e = self.boss.world.ent[0];
        if self.phase == Phase::Dying || !e.on(flag::COLLIDER) || !self.boss.world.active_in_hierarchy(0) {
            return;
        }
        if !crate::polygon_hits_box(s.polygon, world_box(&self.boss.world, 0)) {
            return;
        }
        self.reached = true;
        let p = EnemyParams { invincible: self.invincible, ..self.params };
        match self.health.strike(p, s.damage.clamp(0, 255) as u16) {
            Hit::Damaged | Hit::Killed => {
                self.phase = Phase::Fighting;
                self.recoil_left = psx_math::int32::mul_shr_i32(fight::RECOIL_TIME, 60, 16) as u16 + 1;
                let away = if s.kind == 0 { s.facing } else { 0 };
                self.recoil_vx = away * self.recoil_speed;
                sink(Effect::Struck { hp: self.health.hp as i32 });
                // HealthManager.TakeDamage: `TOOK DAMAGE` to every FSM on the boss object.
                self.send_to_owner_fsms(0, fight::EV_TOOK_DAMAGE);
                if self.health.dead {
                    self.die(sink);
                }
            }
            Hit::Blocked | Hit::Ignored => {}
        }
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn send_to_owner_fsms(&mut self, object: u8, event: u8) {
        for (f, &o) in fight::OWNERS.iter().enumerate() {
            if o == object {
                self.boss.tree().event(f as u8, event);
            }
        }
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// `HealthManager.Die`: the boss object goes, and the corpse prefab is spawned where it stood. The corpse tree takes over
    /// the one tree's memory: nothing of the fight outlives her.
    fn die(&mut self, sink: &mut impl FnMut(Effect)) {
        let at = self.boss.world.world_of(0).0;
        let facing = self.boss.world.ent[0].scale[0];
        let mut pd = self.boss.pd;
        pd[field::DISABLE_PAUSE as usize] = 0;
        self.phase = Phase::Dying;
        self.boss.bind(&corpse::DATA);
        self.boss.pd = pd;
        self.boss.init(self.tick ^ 0x484f_524e);
        // Hornet faces the way she was facing; the corpse's own FSM flings her.
        self.boss.world.ent[0].pos = at;
        self.boss.world.ent[0].scale[0] = facing;
        self.pd_seen = pd;
        self.boss.enter();
        sink(Effect::Died);
    }

    /// The corpse tree's scene objects (drawn while she is dying).
    #[inline(always)]
    pub fn corpse_world(&self) -> &crate::fsm3_world::World {
        &self.boss.world
    }
    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// The corpse's Control state (for tests and the debug overlay).
    pub fn corpse_state(&mut self) -> u8 {
        self.boss.tree().state(0)
    }

    // ---- the corpse -------------------------------------------------------------------------------------------------

    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn step_corpse(&mut self, input: &Input<'_>, count: usize, edge: &dyn Fn(usize) -> [i32; 4], sink: &mut impl FnMut(Effect)) {
        let c = &mut self.boss;
        c.env.hero = [input.hero[0], input.hero[1], 0];
        c.env.camera = input.camera;
        c.env.dt_scaled = input.dt_scaled;
        c.env.dt_real = input.dt_real;
        self.fixed += 50;
        let mut steps = 0;
        while self.fixed >= 60 {
            self.fixed -= 60;
            steps += 1;
        }
        animate(&mut self.boss.world);
        let now = body_step(&mut self.boss.world, self.corpse_contacts, None, count, edge);
        let entered = now & !self.corpse_contacts;
        self.corpse_contacts = now;
        if entered != 0 {
            self.boss.env.contacts = entered;
            self.boss.tree().step(crate::fsm3::PH_COLLISION_ENTER);
        }
        if now != 0 {
            self.boss.env.contacts = now;
            self.boss.tree().step(crate::fsm3::PH_COLLISION_STAY);
        }
        self.boss.env.contacts = 0;
        self.boss.step(steps);
        // The corpse's own events and requests: it sends `HORNET LEAVE` to the Cloak Corpse, plays sounds, flings pieces.
        let n = self.boss.world.req_n as usize;
        for i in 0..n {
            let r = self.boss.world.req[i];
            let (kind, object) = (r[0] as u8, (r[0] >> 8) as u8);
            if kind == k::MESSAGE && r[1] == corpse::EV_HORNET_LEAVE as i32 {
                sink(Effect::CloakLeaves);
                self.phase = Phase::Gone;
            } else if kind == k::MESSAGE && r[1] == corpse::EV_HORNET_KILLED as i32 {
                sink(Effect::GatesOpen);
            } else if kind == k::MESSAGE && (r[1] == corpse::EV_ENEMY_KILL_SHAKE as i32 || r[1] == corpse::EV_AVERAGE_SHAKE as i32) {
                sink(Effect::Shake { average: r[1] == corpse::EV_AVERAGE_SHAKE as i32 });
            } else {
                map_request(r, sink, &self.boss.world, object);
            }
        }
        self.boss.world.req_n = 0;
        self.check_pd(sink);
    }

    // ---- requests and PlayerData ------------------------------------------------------------------------------------

    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn drain(&mut self, sink: &mut impl FnMut(Effect), _input: &Input<'_>) {
        let n = self.boss.world.req_n as usize;
        for i in 0..n {
            let r = self.boss.world.req[i];
            let (kind, object, flags) = (r[0] as u8, (r[0] >> 8) as u8, (r[0] >> 16) as u8);
            if kind == k::INVINCIBLE {
                // SetInvincible [on, from direction]: the HealthManager's flag (the direction filter is the strike's).
                self.invincible = flags & 1 != 0;
            } else if kind == k::MESSAGE && r[1] == fight::EV_BG_CLOSE as i32 {
                sink(Effect::GatesClose);
            } else if kind == k::MESSAGE && r[1] == fight::EV_ENEMY_KILL_SHAKE as i32 {
                sink(Effect::Shake { average: false });
            } else if kind == k::RECOIL {
                self.recoil_speed = r[1];
            } else if kind == k::SPAWN {
                let handle = self.spawn(r[1], [r[2], r[3]]);
                sink(Effect::Spawn { pool: r[1], at: [r[2], r[3]], handle });
            } else if kind == k::DESTROY {
                if let Some(e) = self.boss.world.get_mut(object) {
                    e.set(flag::ACTIVE, false);
                }
                let mut tree = self.boss.tree();
                crate::fsm3b::object::sync_tree(&mut tree);
                sink(Effect::Destroy { object });
            } else {
                map_request(r, sink, &self.boss.world, object);
            }
        }
        self.boss.world.req_n = 0;
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Give a pooled object a free slot of the tree's table: the first inactive object the tree never uses by name.
    fn spawn(&mut self, _pool: i32, at: [i32; 2]) -> u8 {
        let w = &mut self.boss.world;
        w.spawned = NONE8;
        let _ = at;
        w.spawned
    }

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Fields of the PlayerData reserve the pass wrote.
    fn check_pd(&mut self, sink: &mut impl FnMut(Effect)) {
        let (now, seen) = (self.boss.pd, &mut self.pd_seen);
        for f in 0..PD_FIELDS {
            if now[f] != seen[f] {
                seen[f] = now[f];
                sink(Effect::Persist { field: f as u8, value: now[f] });
            }
        }
    }

    // ---- the Knight ---------------------------------------------------------------------------------------------------

    #[cfg_attr(target_arch = "mips", optimize(size))]
    /// Every active DamageHero object of the tree against the Knight.
    fn hurt_hero(&mut self, hero_body: [i32; 4], sink: &mut impl FnMut(Effect)) {
        let w = &self.boss.world;
        for i in 0..w.n {
            let e = &w.ent[i as usize];
            if e.damage == 0 || !e.on(flag::COLLIDER) || !w.active_in_hierarchy(i) {
                continue;
            }
            if overlap(world_box(w, i), hero_body) {
                sink(Effect::HeroHurt { damage: e.damage, from_x: w.world_of(i).0[0] });
                return;
            }
        }
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Object 0's Rigidbody2D against the terrain: its velocity (units a second, set by the FSMs) and gravity scale, the collider
/// it has now. Returns the contact sides (bit 0 up, 1 right, 2 down, 3 left). A kinematic body moves by its velocity alone.
///
/// A sweep of its own rather than `Player::step`: the Knight's solver is a four-kilobyte instantiation per terrain closure,
/// which the resident image has no room for, and a boss only needs the walls, the floor and the ceiling. The rules are
/// the Knight's: vertical edges are walls, any other edge holds from above and from below, with the same lip.
fn body_step(w: &mut crate::fsm3_world::World, was: u8, recoil: Option<i32>, count: usize, edge: &dyn Fn(usize) -> [i32; 4]) -> u8 {
    let e: Ent = w.ent[0];
    if !e.on(flag::ACTIVE) {
        return 0;
    }
    if e.on(flag::KINEMATIC) || !e.on(flag::BODY) {
        let ent = &mut w.ent[0];
        if ent.on(flag::KINEMATIC) {
            ent.pos[0] += ent.vel[0] / 60;
            ent.pos[1] += ent.vel[1] / 60;
        }
        return 0;
    }
    let (bw, bh) = (e.box_size[0].max(ONE / 8), e.box_size[1].max(ONE / 8));
    let (cx, cy) = (e.box_offset[0], e.box_offset[1]);
    let (half, bottom, top) = (bw / 2, cy - bh / 2, cy + bh / 2);
    let (ox, oy) = (e.pos[0] + cx, e.pos[1]);
    let vx = recoil.unwrap_or(e.vel[0]);
    let gravity = psx_math::int32::mul_shr_i32(e.gravity, 60 * ONE, 16);
    let vy = (e.vel[1] - gravity / 60).max(-100 * ONE);
    let mut nx = ox + vx / 60;
    const LIP: i32 = ONE / 20;
    let lip = if was & 4 != 0 { LIP } else { 32 };
    let mut contacts = 0u8;
    for i in 0..count {
        let [x0, y0, x1, y1] = edge(i);
        if y0 == y1 || x0 != x1 || oy + top <= y0.min(y1) + 32 || oy + bottom >= y0.max(y1) - lip {
            continue;
        }
        if nx > ox && ox + half <= x0 && nx + half > x0 {
            nx = x0 - half;
            contacts |= 2;
        }
        if nx < ox && ox - half >= x0 && nx - half < x0 {
            nx = x0 + half;
            contacts |= 8;
        }
    }
    let mut ny = oy + vy / 60;
    let mut vy = vy;
    for i in 0..count {
        let [x0, y0, x1, y1] = edge(i);
        if x0 == x1 || nx + half <= x0.min(x1) + 32 || nx - half >= x0.max(x1) - 32 {
            continue;
        }
        let height = |x: i32| {
            if y0 == y1 {
                y0
            } else {
                y0 + psx_math::int32::mul_div_i32(x.clamp(x0.min(x1), x0.max(x1)) - x0, y1 - y0, x1 - x0)
            }
        };
        let (old_surface, surface) = (height(ox), height(nx));
        if ny < oy && oy + bottom >= old_surface - ONE / 20 && ny + bottom <= surface {
            ny = surface - bottom;
            vy = 0;
            contacts |= 4;
        }
        if ny > oy && oy + top <= old_surface && ny + top >= surface {
            ny = surface - top;
            vy = 0;
            contacts |= 1;
        }
    }
    // Resting on the floor: gravity's small step keeps the body in contact.
    if was & 4 != 0 && contacts & 5 == 0 && e.vel[1] <= 0 && vy <= 0 && ny >= oy - 2 {
        contacts |= 4;
    }
    let ent = &mut w.ent[0];
    ent.pos[0] = nx - cx;
    ent.pos[1] = ny;
    ent.vel[1] = vy;
    if contacts & 10 != 0 && recoil.is_none() {
        ent.vel[0] = 0;
    }
    contacts
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// One tick of every sprite animator of a tree: the clip's clock (`clip_from` counts the ticks since it started), the frame
/// triggers it passes and the end of a clip that ends.
fn animate(w: &mut crate::fsm3_world::World) {
    for i in 0..w.n {
        let info = {
            let e = &w.ent[i as usize];
            if e.clip == crate::fsm3_world::NO_CLIP || e.frame >= 0 {
                continue;
            }
            w.clip_info(i, e.clip)
        };
        let e = &mut w.ent[i as usize];
        if let Some(c) = info {
            if c.trig.contains(&e.clip_from) {
                e.anim_event = true;
            }
            e.clip_from = if !c.once && e.clip_from + 1 >= c.len { 0 } else { e.clip_from.saturating_add(1) };
        }
        if e.anim_left != 0 && e.anim_left != 0xffff {
            e.anim_left -= 1;
        }
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// The requests with nothing of the fight's state in them become effects as they are.
fn map_request(r: [i32; 4], sink: &mut impl FnMut(Effect), _w: &crate::fsm3_world::World, object: u8) {
    let (kind, flags) = (r[0] as u8, (r[0] >> 16) as u8);
    let (a, b, c) = (r[1], r[2], r[3]);
    if kind == k::AUDIO {
        sink(Effect::Audio { object, clip: a, volume: b, pitch: c, looped: flags & 2 != 0 });
    } else if kind == k::AUDIO_STOP {
        sink(Effect::AudioStop { object });
    } else if kind == k::MUSIC {
        sink(Effect::Music { cue: a, delay: b, transition: c });
    } else if kind == k::SNAPSHOT {
        sink(Effect::Snapshot { id: a });
    } else if kind == k::PARTICLES {
        sink(Effect::Particles { object, on: flags & 1 != 0, emit: a });
    } else if kind == k::MESSAGE {
        sink(Effect::Message { object, event: a, ticks: b, mode: flags as i32 });
    } else if kind == k::CONVERSATION {
        sink(Effect::Conversation { id: a });
    } else if kind == k::HERO {
        sink(Effect::HeroControl { regain: flags & 1 != 0 });
    } else if kind == k::CALL {
        sink(classify_call(object, a, b, c));
    } else if kind == k::SPAWN_RANDOM {
        sink(Effect::Fling { prefab: a, count: b, speed: c });
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// A native component call by the names the cooker gave the behaviour and method: the fight's string ids (the corpse's table
/// has the same names under its own ids, but it calls only `GameManager.AwardAchievement`, which is nothing here).
fn classify_call(object: u8, behaviour: i32, method: i32, arg: i32) -> Effect {
    if behaviour == fight::S_HERO_CONTROLLER && method == fight::S_RELINQUISH_CONTROL {
        Effect::HeroControl { regain: false }
    } else if behaviour == fight::S_HERO_CONTROLLER && method == fight::S_REGAIN_CONTROL {
        Effect::HeroControl { regain: true }
    } else if behaviour == fight::S_DIALOGUE_BOX && method == fight::S_START_CONVERSATION {
        Effect::Conversation { id: arg }
    } else {
        Effect::Call { object, behaviour, method, arg }
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// The world box of object `i`'s collider: its centre (transform plus the offset scaled by the world scale) and its size.
fn world_box(w: &crate::fsm3_world::World, i: u8) -> [i32; 4] {
    let e = &w.ent[i as usize];
    let (pos, scale, _) = w.world_of(i);
    let (sx, sy) = (scale[0].abs(), scale[1].abs());
    let off = [mul(e.box_offset[0], scale[0]), mul(e.box_offset[1], scale[1])];
    let half = [mul(e.box_size[0], sx) / 2, mul(e.box_size[1], sy) / 2];
    let c = [pos[0] + off[0], pos[1] + off[1]];
    [c[0] - half[0], c[1] - half[1], c[0] + half[0], c[1] + half[1]]
}

#[cfg_attr(target_arch = "mips", optimize(size))]
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[cfg_attr(target_arch = "mips", optimize(size))]
fn mul(a: i32, b: i32) -> i32 {
    psx_math::int32::mul_shr_i32(a, b, 16)
}
