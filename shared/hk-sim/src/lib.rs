//! Bounded deterministic chamber movement. Q16.16 world units, 60 Hz steps.
#![no_std]
use hk_format::Room;
pub mod acid_flyer;
mod actors;
pub mod aspid;
pub mod baldur;
pub mod blocker;
pub mod boss;
pub mod buzz;
pub mod climber;
mod combat;
mod corpse;
pub mod false_knight;
pub mod gruz_mother;
pub mod gruzzer;
pub mod hatcher;
pub mod husk_guard;
pub mod mawlek;
pub mod mosquito;
pub mod moss_walker;
pub mod pigeon;
pub mod runner;
pub mod runner_senses;
pub mod shade;
pub mod shockwave;
pub mod vengefly;
pub mod zombie_shield;
pub use corpse::{Corpse, CorpsePhase, CorpseSpec};
mod focus;
mod vitals;
pub use focus::{Focus, FocusClip, FocusEvents, FocusInput, FocusParams};
pub mod script;
mod spell;
pub use spell::{Cast, CastPhase, Fireball, FireballParams};
mod dream_nail;
pub use dream_nail::{DreamNail, DreamNailParams, DreamPhase};
mod nail_response;
pub mod persistent;
pub mod waves;
pub use actors::{
    persistent_actor, resolve_actor_spawn, walker_senses, ActorController, ActorHealth,
    ActorPlacement, ActorSpec, EnemyParams, Hit, PersistentActor, WalkParams, WalkState,
    MAX_ACTORS, SEMI_PERSISTENT,
};
pub use combat::polygon_hits_box;
pub use combat::{AttackParams, Grass, Nail};
pub use nail_response::{NailResponse, NailResponseParams};
pub use vitals::{Hurt, VitalParams, Vitals, PULSE_TICKS};
pub const ONE: i32 = 65536;
/// Two terrain edges (or boxes) are equal. The derived `==` on `[i32; 4]` is a
/// 16-byte `memcmp` call on the R3000, a byte loop; this is four word compares.
#[inline(always)]
pub fn same_edge(a: &[i32; 4], b: &[i32; 4]) -> bool {
    ((a[0] ^ b[0]) | (a[1] ^ b[1]) | (a[2] ^ b[2]) | (a[3] ^ b[3])) == 0
}
/// A removed terrain edge (all zero), without a `memcmp` call.
#[inline(always)]
pub fn empty_edge(e: &[i32; 4]) -> bool {
    (e[0] | e[1] | e[2] | e[3]) == 0
}
#[derive(Clone, Copy)]
pub struct Params {
    pub speed: i32,
    pub jump: i32,
    pub gravity: i32,
    pub fall: i32,
    pub hold_ticks: u16,
    pub min_ticks: u16,
    /// A jump pressed this many ticks before landing still fires, as the
    /// source's `JUMP_QUEUE_STEPS` buffer does.
    pub jump_queue_ticks: u16,
    /// Mothwing Cloak: constant velocity for `dash_ticks` with gravity off.
    pub dash_speed: i32,
    pub dash_ticks: u16,
    pub dash_cooldown_ticks: u16,
    pub dash_queue_ticks: u16,
    /// Shade Cloak: the upgraded dash keeps the ordinary speed and duration and
    /// adds invulnerability, gated by its own cooldown rather than the dash's.
    pub shadow_dash_cooldown_ticks: u16,
    /// Mantis Claw: the fall speed a wall slide clamps to (negative).
    pub wallslide_speed: i32,
    /// Holding away from the wall for this long detaches (`WALL_STICKY_STEPS`).
    pub wall_sticky_ticks: u16,
    /// Wall jump kickoff, decaying by `walljump_decel` for every locked tick.
    pub walljump_speed: i32,
    pub walljump_decel: i32,
    /// Pressing back releases the lock after `wall_lock_short`; it always ends
    /// after `wall_lock_long`.
    pub wall_lock_short: u16,
    pub wall_lock_long: u16,
    /// Monarch Wings: `JUMP_SPEED * 1.1`, held from `double_jump_delay_ticks`
    /// to `double_jump_ticks` while the wings beat.
    pub double_jump_speed: i32,
    pub double_jump_delay_ticks: u16,
    pub double_jump_ticks: u16,
    pub double_jump_queue_ticks: u16,
    /// Crystal Heart. The travel speed is the Superdash FSM's own variable,
    /// which overrides the HeroController's unread SUPER_DASH_SPEED.
    pub super_dash_speed: i32,
    pub super_dash_charge_ticks: u16,
    /// The travel cannot be cancelled for this long; a wall ends it either way.
    pub super_dash_cancel_ticks: u16,
    pub super_dash_recover_ticks: u16,
    /// `LEDGE_BUFFER_STEPS`: a jump still fires this long after walking off a
    /// ledge, and `HEAD_BUMP_STEPS` refuses one this long after a ceiling.
    pub ledge_buffer_ticks: u16,
    pub head_bump_ticks: u16,
    pub half_width: i32,
    pub bottom: i32,
    pub top: i32,
    /// `SHROOM_BOUNCE_VELOCITY`: the one-shot rise a BounceShroom answers a
    /// down slash with. `BOUNCE_SHROOM_TIME` is zero, so there is no hold.
    pub shroom_speed: i32,
}
/// Wall interaction, double jump and super dash, added by P14 step 2.
impl Params {
    /// Every field zero. The enemy, corpse and prop bodies reuse `Player::step`
    /// for its collision and only set the handful of fields they care about,
    /// so they spread this instead of restating the hero's ability constants.
    pub const ZERO: Self = Self {
        speed: 0,
        jump: 0,
        gravity: 0,
        fall: 0,
        hold_ticks: 0,
        min_ticks: 0,
        jump_queue_ticks: 0,
        dash_speed: 0,
        dash_ticks: 0,
        dash_cooldown_ticks: 0,
        dash_queue_ticks: 0,
        shadow_dash_cooldown_ticks: 0,
        wallslide_speed: 0,
        wall_sticky_ticks: 0,
        walljump_speed: 0,
        walljump_decel: 0,
        wall_lock_short: 0,
        wall_lock_long: 0,
        double_jump_speed: 0,
        double_jump_delay_ticks: 0,
        double_jump_ticks: 0,
        double_jump_queue_ticks: 0,
        super_dash_speed: 0,
        super_dash_charge_ticks: 0,
        super_dash_cancel_ticks: 0,
        super_dash_recover_ticks: 0,
        ledge_buffer_ticks: 0,
        head_bump_ticks: 0,
        half_width: 0,
        bottom: 0,
        top: 0,
        shroom_speed: 0,
    };
}
/// Crystal Heart, as the Hero's Superdash FSM runs it: hold to charge against
/// the ground or a wall, release once charged to travel, and stop at a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuperDash {
    Off,
    Charging(u16),
    Ready,
    Travelling(u16),
    /// The Hit Wall recovery, during which the Knight has no control.
    Recovering(u16),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Player {
    pub x: i32,
    pub y: i32,
    pub vy: i32,
    pub grounded: bool,
    pub facing: i32,
    pub jump_tick: u16,
    pub jumping: bool,
    pub was_jump: bool,
    pub jump_queuing: bool,
    pub jump_queue: u16,
    /// Ticks of dash left; the dash owns velocity and suspends gravity while non-zero.
    pub dash_left: u16,
    pub dash_cooldown: u16,
    /// One dash per airtime, as `CanDash` enforces through `airDashed`.
    pub air_dashed: bool,
    pub dash_queuing: bool,
    pub dash_queue: u16,
    pub was_dash: bool,
    /// Set when the Mothwing Cloak has been acquired; nothing grants it yet.
    pub has_dash: bool,
    /// Source cState.shadowDashing: TakeDamage returns immediately while set.
    pub shadow_dashing: bool,
    pub shadow_dash_cooldown: u16,
    pub has_shade_cloak: bool,
    /// -1 for a wall on the left, +1 on the right, 0 for none. Airborne only,
    /// since nothing reads a grounded wall contact.
    pub touching_wall: i32,
    pub wall_sliding: bool,
    /// Ticks spent holding away from the wall; `WALL_STICKY_STEPS` detaches.
    pub wall_unstick: u16,
    /// While locked the wall jump owns horizontal velocity: +1 kicked right.
    pub wall_locked: bool,
    pub wall_jumped: i32,
    pub wall_lock_ticks: u16,
    pub walljump_speed: i32,
    pub has_walljump: bool,
    pub double_jumping: bool,
    pub double_jumped: bool,
    pub double_jump_tick: u16,
    pub has_double_jump: bool,
    pub super_dash: SuperDash,
    pub has_super_dash: bool,
    /// Source ledgeBufferSteps and headBumpSteps, both counted down per tick.
    pub ledge_buffer: u16,
    pub head_bump: u16,
    /// Source cState.shroomBouncing: held for the rise a BounceShroom started,
    /// during which HeroController.Bounce refuses an ordinary pogo.
    pub shroom_bouncing: bool,
    /// Set by `Nail::tick` so the source's CanDash can refuse a dash during an
    /// attack's active window, as it reads cState.attacking there.
    pub attack_recovering: bool,
    pub animation: u16,
    pub animation_tick: u32,
    pub land_tick: u16,
}
impl Player {
    pub const fn spawn(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            vy: 0,
            grounded: false,
            facing: 1,
            jump_tick: 0,
            jumping: false,
            was_jump: false,
            jump_queuing: false,
            jump_queue: 0,
            dash_left: 0,
            dash_cooldown: 0,
            air_dashed: false,
            dash_queuing: false,
            dash_queue: 0,
            was_dash: false,
            has_dash: false,
            shadow_dashing: false,
            shadow_dash_cooldown: 0,
            has_shade_cloak: false,
            touching_wall: 0,
            wall_sliding: false,
            wall_unstick: 0,
            wall_locked: false,
            wall_jumped: 0,
            wall_lock_ticks: 0,
            walljump_speed: 0,
            has_walljump: false,
            double_jumping: false,
            double_jumped: false,
            double_jump_tick: 0,
            has_double_jump: false,
            super_dash: SuperDash::Off,
            has_super_dash: false,
            ledge_buffer: 0,
            head_bump: 0,
            shroom_bouncing: false,
            attack_recovering: false,
            animation: 0,
            animation_tick: 0,
            land_tick: 0,
        }
    }
    pub fn tick(&mut self, p: Params, dir: i32, jump: bool, room: &Room) {
        self.step(p, dir, jump, room.counts[5], |i| room.edge(i));
    }
    /// `CanDash` plus the source's `DASH_QUEUE_STEPS` buffer. Call once per
    /// tick before `step`; a caller that never calls it never dashes.
    pub fn dash_input(&mut self, p: Params, pressed: bool) {
        self.dash_cooldown = self.dash_cooldown.saturating_sub(1);
        self.shadow_dash_cooldown = self.shadow_dash_cooldown.saturating_sub(1);
        if pressed && !self.was_dash {
            self.dash_queuing = true;
            self.dash_queue = 0;
        }
        self.was_dash = pressed;
        if !self.dash_queuing {
            return;
        }
        // A dash in the air is allowed once until the next landing.
        // CanDash: the cooldown, no dash running, not inside an attack's
        // recovery window, and either footing, an unspent air dash or a wall.
        let allowed = self.has_dash
            && self.dash_left == 0
            && self.dash_cooldown == 0
            && !self.attack_recovering
            && (self.grounded || !self.air_dashed || self.wall_sliding);
        if pressed && allowed {
            self.dash_left = p.dash_ticks;
            self.dash_cooldown = p.dash_cooldown_ticks;
            // HeroDash takes the shadow branch whenever the Cloak is owned and
            // its own timer has run out; otherwise this is an ordinary dash.
            self.shadow_dashing = self.has_shade_cloak && self.shadow_dash_cooldown == 0;
            if self.shadow_dashing {
                self.shadow_dash_cooldown = p.shadow_dash_cooldown_ticks;
            }
            self.dash_queuing = false;
            if !self.grounded {
                self.air_dashed = true;
            }
            // Dash owns velocity outright, and gravity is off for its duration.
            self.vy = 0;
            self.jumping = false;
        } else if !pressed || self.dash_queue >= p.dash_queue_ticks {
            self.dash_queuing = false;
        } else {
            self.dash_queue += 1;
        }
    }
    /// `HeroController::ShroomBounce`, which NailSlash's down slash calls in
    /// place of the ordinary bounce when the target carries a BounceShroom.
    /// Both air abilities come back and the rise replaces the velocity
    /// outright; there is no timer, so the rise simply runs out under gravity.
    pub fn shroom_bounce(&mut self, p: Params) {
        self.double_jumped = false;
        self.air_dashed = false;
        self.shroom_bouncing = true;
        // The source leaves cState.jumping alone here, but this port re-pins
        // the jump hold's velocity every tick, so an unfinished hold would eat
        // the impulse. The ordinary pogo ends the hold for the same reason.
        self.jumping = false;
        self.vy = p.shroom_speed;
    }
    /// The Superdash FSM's button, sampled once per tick before `step`.
    /// Charging needs the ground or a wall slide, letting go early cancels,
    /// and letting go once charged launches along the facing.
    pub fn super_dash_input(&mut self, p: Params, held: bool) {
        self.super_dash = match self.super_dash {
            SuperDash::Off
                if held && self.has_super_dash && (self.grounded || self.wall_sliding) =>
            {
                // The entry call is the charge's first tick.
                SuperDash::Charging(1)
            }
            // Ground Charge also watches Y Speed < -0.1, so walking off the
            // ledge being charged on drops the charge.
            SuperDash::Charging(_) if !held || (!self.grounded && !self.wall_sliding) => {
                SuperDash::Off
            }
            SuperDash::Charging(t) if t + 1 >= p.super_dash_charge_ticks => SuperDash::Ready,
            SuperDash::Charging(t) => SuperDash::Charging(t + 1),
            SuperDash::Ready if !held => {
                self.vy = 0;
                SuperDash::Travelling(0)
            }
            other => other,
        };
    }
    pub fn step(
        &mut self,
        p: Params,
        dir: i32,
        jump: bool,
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) {
        let dir = dir.clamp(-1, 1);
        if dir != 0 {
            self.facing = dir;
        }
        let jump_pressed = jump && !self.was_jump;
        self.ledge_buffer = self.ledge_buffer.saturating_sub(1);
        self.head_bump = self.head_bump.saturating_sub(1);
        // Source: pressing back out of a wall jump releases the lock once
        // WJLOCK_STEPS_SHORT have passed; otherwise it runs its full length.
        if self.wall_locked && dir == -self.wall_jumped && self.wall_lock_ticks >= p.wall_lock_short
        {
            self.wall_locked = false;
        }
        // Source: CanWallSlide wants the Claw, airtime, no dash and a fall
        // (or a slide already running); entry also wants the wall held into.
        // WALL_STICKY_STEPS keeps the slide when the stick is let go, and only
        // holding away for that long lets go of the wall.
        if self.wall_sliding {
            if dir == -self.touching_wall {
                self.wall_unstick += 1;
            } else {
                self.wall_unstick = 0;
            }
            if self.grounded
                || self.dash_left > 0
                || self.touching_wall == 0
                || self.wall_unstick >= p.wall_sticky_ticks
            {
                self.wall_sliding = false;
            }
        } else if self.has_walljump
            && !self.grounded
            && self.dash_left == 0
            && !self.double_jumping
            && self.vy < 0
            && self.touching_wall != 0
            && dir == self.touching_wall
        {
            self.wall_sliding = true;
            self.wall_unstick = 0;
            // The slide restores both air abilities and turns into the wall.
            self.air_dashed = false;
            self.double_jumped = false;
            self.facing = self.touching_wall;
        }
        // Source: a jump press sets jumpQueuing and each step retries while
        // jumpQueueSteps <= JUMP_QUEUE_STEPS, so a press up to that many steps
        // early is not lost. A strict edge dropped those.
        if jump && !self.was_jump {
            self.jump_queuing = true;
            self.jump_queue = 0;
        }
        if self.jump_queuing {
            // Source order: CanWallJump, then CanJump, then CanDoubleJump.
            let can_wall_jump = self.has_walljump
                && (self.wall_sliding || (self.touching_wall != 0 && !self.grounded));
            // CanJump: not dashing, not already jumping, no head bump, and
            // either on the ground or still inside the ledge buffer.
            let can_jump = (self.grounded || self.ledge_buffer > 0)
                && !self.jumping
                && self.dash_left == 0
                && self.head_bump == 0
                && self.jump_queue <= p.jump_queue_ticks;
            let can_double_jump = self.has_double_jump
                && !self.double_jumped
                && !self.grounded
                && self.dash_left == 0;
            // The retry window is the longer of the two the source keeps.
            let window = if self.has_double_jump {
                p.double_jump_queue_ticks.max(p.jump_queue_ticks)
            } else {
                p.jump_queue_ticks
            };
            if jump && can_wall_jump {
                self.facing = -self.touching_wall;
                self.wall_jumped = -self.touching_wall;
                self.wall_sliding = false;
                self.touching_wall = 0;
                self.air_dashed = false;
                self.double_jumped = false;
                self.walljump_speed = p.walljump_speed;
                self.wall_locked = true;
                self.wall_lock_ticks = 0;
                self.jumping = true;
                self.jump_tick = 0;
                self.grounded = false;
                self.land_tick = 0;
                self.jump_queuing = false;
            } else if jump && can_jump {
                self.jumping = true;
                self.jump_tick = 0;
                self.grounded = false;
                self.ledge_buffer = 0;
                self.land_tick = 0;
                self.jump_queuing = false;
            } else if jump && can_double_jump {
                self.jumping = false;
                self.double_jumping = true;
                self.double_jumped = true;
                self.double_jump_tick = 0;
                self.jump_queuing = false;
            } else if !jump || self.jump_queue >= window {
                self.jump_queuing = false;
            } else {
                self.jump_queue += 1;
            }
        }
        if self.jumping {
            if self.jump_tick < p.hold_ticks && (jump || self.jump_tick < p.min_ticks) {
                self.vy = p.jump;
                self.jump_tick += 1;
            } else {
                self.jumping = false;
                if !jump && self.vy > 0 {
                    self.vy = 0;
                }
            }
        } else if self.double_jumping {
            // Source DoubleJump(): the first steps are the wing flourish, then
            // JUMP_SPEED * 1.1 is held until DOUBLE_JUMP_STEPS runs out. A
            // release does not cancel it.
            if self.double_jump_tick > p.double_jump_ticks || self.grounded {
                self.double_jumping = false;
            } else {
                if self.double_jump_tick > p.double_jump_delay_ticks {
                    self.vy = p.double_jump_speed;
                }
                self.double_jump_tick += 1;
            }
        } else if !jump && self.was_jump && self.vy > 0 {
            self.vy = 0;
        }
        self.was_jump = jump;
        if self.grounded {
            self.air_dashed = false;
        }
        let super_dashing = matches!(self.super_dash, SuperDash::Travelling(_));
        // Relinquish Control: a charge, a charged hold and the Hit Wall
        // recovery all pin the Knight horizontally, but gravity keeps running
        // so a ground charge stays on its floor.
        let pinned = matches!(
            self.super_dash,
            SuperDash::Charging(_) | SuperDash::Ready | SuperDash::Recovering(_)
        );
        let dashing = self.dash_left > 0;
        let (dir, speed) = if super_dashing {
            // SetGravity2dScale(0): the travel owns velocity outright.
            self.vy = 0;
            (self.facing, p.super_dash_speed)
        } else if dashing {
            self.dash_left -= 1;
            if self.dash_left == 0 {
                self.shadow_dashing = false;
            }
            // AffectedByGravity(false): the dash holds its own velocity.
            self.vy = 0;
            (self.facing, p.dash_speed)
        } else {
            self.vy = (self.vy - p.gravity / 60).max(-p.fall);
            // WALLSLIDE_DECEL is zero in the source, so the slide is a clamp.
            if self.wall_sliding {
                self.vy = self.vy.max(p.wallslide_speed);
            }
            if pinned {
                (0, 0)
            } else if self.wall_locked {
                let locked = (self.wall_jumped, self.walljump_speed);
                self.wall_lock_ticks += 1;
                if self.wall_lock_ticks > p.wall_lock_long {
                    self.wall_locked = false;
                }
                self.walljump_speed -= p.walljump_decel;
                locked
            } else {
                (dir, p.speed)
            }
        };
        let ox = self.x;
        let oy = self.y;
        let mut nx = ox + dir * speed / 60;
        // Axis-separated swept AABB against source terrain edges, with open endpoints.
        // No invented floor: collision comes only from cooked source edges.
        //
        // A grounded body walks over a lip no taller than STEP_LIP, the same
        // tolerance the floor pass below snaps up by: King's Pass has two box
        // platforms whose tops sit 0.006 units under the floor they meet
        // (level6:8139 against 8357 at x130, 8261 against 8356 at x147), which
        // Box2D's rounded contacts pass and which stopped the Knight dead.
        const STEP_LIP: i32 = ONE / 20;
        let lip = if self.grounded { STEP_LIP } else { 32 };
        for i in 0..count {
            let [x0, y0, x1, y1] = edge(i);
            if y0 == y1
                || x0 != x1
                || oy + p.top <= y0.min(y1) + 32
                || oy + p.bottom >= y0.max(y1) - lip
            {
                continue;
            }
            if nx > ox && ox + p.half_width <= x0 && nx + p.half_width > x0 {
                nx = x0 - p.half_width;
            }
            if nx < ox && ox - p.half_width >= x0 && nx - p.half_width < x0 {
                nx = x0 + p.half_width;
            }
        }
        let mut ny = oy + self.vy / 60;
        let was_ground = self.grounded;
        self.grounded = false;
        for i in 0..count {
            let [x0, y0, x1, y1] = edge(i);
            if x0 == x1
                || nx + p.half_width <= x0.min(x1) + 32
                || nx - p.half_width >= x0.max(x1) - 32
            {
                continue;
            }
            // Sample the authored segment under the character centre. Clamp at
            // endpoints while the box overlaps so the floor does not develop
            // gaps between a ramp and its adjoining horizontal segment.
            // A flat edge's height is y0 everywhere (mul_div_i32(_, 0, _) is 0),
            // without the 64-bit multiply and divide.
            let height = |x: i32| {
                if y0 == y1 {
                    y0
                } else {
                    y0 + psx_math::int32::mul_div_i32(
                        x.clamp(x0.min(x1), x0.max(x1)) - x0,
                        y1 - y0,
                        x1 - x0,
                    )
                }
            };
            let old_surface = height(ox);
            let surface = height(nx);
            let follows_slope = was_ground
                && !self.jumping
                && y0 != y1
                && (oy + p.bottom - old_surface).abs() <= ONE / 20;
            if (ny < oy || follows_slope)
                && oy + p.bottom >= old_surface - ONE / 20
                && (ny + p.bottom <= surface || follows_slope)
            {
                ny = surface - p.bottom;
                self.vy = 0;
                self.grounded = true;
                self.jumping = false;
            }
            if ny > oy && oy + p.top <= old_surface && ny + p.top >= surface {
                ny = surface - p.top;
                self.vy = 0;
                self.jumping = false;
                self.head_bump = p.head_bump_ticks;
            }
        }
        // Source CheckTouchingWall is a raycast, so contact holds whether or
        // not the wall is being pushed into. A probe just outside each side
        // reproduces that against the same vertical edges the sweep uses.
        // Only the Claw reads it, and every enemy body reuses this step, so
        // the extra edge sweep is skipped for everything that cannot wall jump.
        self.touching_wall = 0;
        if self.has_walljump && !self.grounded {
            const PROBE: i32 = ONE / 16;
            for i in 0..count {
                let [x0, y0, x1, y1] = edge(i);
                if x0 != x1
                    || y0 == y1
                    || ny + p.top <= y0.min(y1) + 32
                    || ny + p.bottom >= y0.max(y1) - 32
                {
                    continue;
                }
                if (nx + p.half_width - x0).abs() <= PROBE {
                    self.touching_wall = 1;
                }
                if (nx - p.half_width - x0).abs() <= PROBE {
                    self.touching_wall = -1;
                }
            }
        }
        // Source: leaving the ground without jumping arms the ledge buffer.
        if was_ground && !self.grounded && !self.jumping {
            self.ledge_buffer = p.ledge_buffer_ticks;
        }
        if self.grounded {
            self.wall_sliding = false;
            self.double_jumped = false;
            self.double_jumping = false;
        }
        // Source Update: the shroom flag lasts exactly as long as the rise it
        // started, and a ceiling or a floor ends that rise as surely as gravity.
        if self.shroom_bouncing && self.vy <= 0 {
            self.shroom_bouncing = false;
        }
        self.super_dash = match self.super_dash {
            // Hit Wall: the sweep refused part of the step, so a wall is there.
            SuperDash::Travelling(_) if nx != ox + dir * speed / 60 => {
                SuperDash::Recovering(p.super_dash_recover_ticks)
            }
            // Cancelable: once the locked window passes, a jump gets out of it.
            // The source consumes that press; here it also feeds the jump queue.
            SuperDash::Travelling(t) if t >= p.super_dash_cancel_ticks && jump_pressed => {
                SuperDash::Off
            }
            SuperDash::Travelling(t) => SuperDash::Travelling(t + 1),
            SuperDash::Recovering(t) if t <= 1 => SuperDash::Off,
            SuperDash::Recovering(t) => SuperDash::Recovering(t - 1),
            other => other,
        };
        self.x = nx;
        self.y = ny;
        if self.grounded && !was_ground {
            self.land_tick = 1;
        } else if self.land_tick > 0 {
            self.land_tick += 1;
            if self.land_tick > 15 {
                self.land_tick = 0;
            }
        }
        let anim = if !self.grounded {
            if self.vy > 0 {
                2
            } else {
                3
            }
        } else if dir != 0 {
            1
        } else if self.land_tick > 0 {
            4
        } else {
            0
        };
        if anim != self.animation {
            self.animation = anim;
            self.animation_tick = 0;
        } else {
            self.animation_tick = self.animation_tick.wrapping_add(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> Params {
        Params {
            speed: 8 * ONE,
            jump: 16 * ONE,
            gravity: 48 * ONE,
            fall: 20 * ONE,
            hold_ticks: 12,
            min_ticks: 5,
            half_width: ONE / 4,
            bottom: -ONE,
            top: ONE / 4,
            ..Params::ZERO
        }
    }
    #[test]
    fn swept_floor_wall_and_repeatable_route() {
        let p = params();
        let edges = [[-10 * ONE, 0, 10 * ONE, 0], [3 * ONE, 0, 3 * ONE, 6 * ONE]];
        let route = || {
            let mut a = Player::spawn(0, ONE);
            for t in 0..180 {
                a.step(p, 1, (10..25).contains(&t), 2, |i| edges[i]);
            }
            a
        };
        let a = route();
        assert_eq!(a, route());
        assert_eq!(a.y, ONE);
        assert_eq!(a.x, 3 * ONE - p.half_width);
        assert!(a.grounded);
    }
    #[test]
    fn short_jump_lower_than_held_jump() {
        let apex = |hold| {
            let mut a = Player::spawn(0, ONE);
            let mut high = 0;
            for t in 0..120 {
                a.step(params(), 0, (1..hold).contains(&t), 1, |_| {
                    [-10 * ONE, 0, 10 * ONE, 0]
                });
                high = high.max(a.y);
            }
            high
        };
        assert!(apex(30) > apex(3) + ONE);
    }
    #[test]
    fn no_synthetic_floor() {
        let mut a = Player::spawn(0, ONE);
        for _ in 0..120 {
            a.step(params(), 0, false, 0, |_| [0; 4]);
        }
        assert!(a.y < -10 * ONE);
    }
    #[test]
    fn source_marker_small_overlap_is_resolved() {
        let mut player = Player::spawn(0, ONE - ONE / 40);
        player.step(params(), 0, false, 1, |_| [-10 * ONE, 0, 10 * ONE, 0]);
        assert_eq!(player.y, ONE);
        assert!(player.grounded);
    }
    /// King's Pass at x130: a box platform whose top is 0.006 under the floor
    /// it meets, and the floor collider's own vertical face under that floor.
    fn lip_room(lip: i32) -> [[i32; 4]; 3] {
        [
            [-10 * ONE, -lip, 2 * ONE, -lip],
            [2 * ONE, 0, 10 * ONE, 0],
            [2 * ONE, -2 * ONE, 2 * ONE, 0],
        ]
    }
    #[test]
    fn a_grounded_walk_passes_a_lip_the_floor_snap_climbs() {
        let room = lip_room(393);
        let mut a = Player::spawn(0, ONE - 393);
        a.step(params(), 0, false, 3, |i| room[i]);
        assert!(a.grounded);
        for _ in 0..60 {
            a.step(params(), 1, false, 3, |i| room[i]);
        }
        assert!(a.x > 4 * ONE, "stopped at x {}", a.x);
        assert!(a.grounded);
        assert_eq!(a.y, ONE);
    }
    #[test]
    fn a_step_taller_than_the_lip_still_blocks() {
        let room = lip_room(ONE / 5);
        let mut a = Player::spawn(0, ONE - ONE / 5);
        a.step(params(), 0, false, 3, |i| room[i]);
        for _ in 0..60 {
            a.step(params(), 1, false, 3, |i| room[i]);
        }
        assert_eq!(a.x, 2 * ONE - params().half_width);
    }
    #[test]
    fn authored_ramp_can_be_walked_up_and_down() {
        let p = params();
        let mut a = Player::spawn(ONE, 2 * ONE);
        a.grounded = true;
        let ramp = [0, 0, 10 * ONE, 10 * ONE];
        for _ in 0..30 {
            a.step(p, 1, false, 1, |_| ramp);
        }
        assert!(a.grounded);
        assert!((a.y - a.x - ONE).abs() < 64);
        for _ in 0..30 {
            a.step(p, -1, false, 1, |_| ramp);
        }
        assert!(a.grounded);
        assert!((a.x - ONE).abs() < 64);
        assert!((a.y - 2 * ONE).abs() < 64);
    }
}

#[cfg(test)]
mod dash_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        half_width: ONE / 2,
        bottom: -ONE,
        dash_speed: 20 * ONE,
        dash_ticks: 15,
        dash_cooldown_ticks: 36,
        dash_queue_ticks: 10,
        ..Params::ZERO
    };
    fn flat() -> impl Fn(usize) -> [i32; 4] {
        |_| [0, 0, 0, 0]
    }
    fn hero() -> Player {
        let mut p = Player::spawn(0, 0);
        p.has_dash = true;
        p.grounded = true;
        p
    }
    #[test]
    fn without_the_cloak_the_button_does_nothing() {
        let mut p = Player::spawn(0, 0);
        p.grounded = true;
        p.dash_input(P, true);
        assert_eq!(p.dash_left, 0);
    }
    #[test]
    fn a_dash_runs_for_its_time_at_its_own_speed_and_then_stops() {
        let mut p = hero();
        p.facing = 1;
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks);
        let start = p.x;
        for _ in 0..P.dash_ticks {
            p.step(P, 0, false, 0, flat());
        }
        // Constant velocity for the whole dash, regardless of the held direction.
        assert_eq!(p.x - start, P.dash_speed / 60 * P.dash_ticks as i32);
        assert_eq!(p.dash_left, 0);
        assert_eq!(p.vy, 0, "gravity is suspended for the dash");
    }
    #[test]
    fn the_cooldown_blocks_a_second_dash_until_it_expires() {
        let mut p = hero();
        p.dash_input(P, true);
        for _ in 0..P.dash_ticks {
            p.step(P, 0, false, 0, flat());
        }
        p.dash_input(P, false);
        p.dash_input(P, true);
        assert_eq!(p.dash_left, 0, "still cooling down");
        for _ in 0..P.dash_cooldown_ticks {
            p.dash_input(P, false);
        }
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks);
    }
    #[test]
    fn only_one_dash_per_airtime() {
        let mut p = hero();
        p.grounded = false;
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks, "the first air dash is allowed");
        assert!(p.air_dashed);
        for _ in 0..P.dash_ticks {
            p.step(P, 0, false, 0, flat());
        }
        for _ in 0..P.dash_cooldown_ticks {
            p.dash_input(P, false);
        }
        p.dash_input(P, true);
        assert_eq!(
            p.dash_left, 0,
            "a second dash in the same airtime is refused"
        );
        // Touching down restores it.
        p.grounded = true;
        p.step(P, 0, false, 0, flat());
        p.dash_input(P, false);
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks, "landing clears airDashed");
    }
    #[test]
    fn a_dash_pressed_before_landing_is_buffered() {
        let mut p = hero();
        p.grounded = false;
        p.air_dashed = true;
        p.dash_input(P, true);
        assert_eq!(p.dash_left, 0);
        // Still held, and the Knight lands inside the queue window.
        for _ in 0..P.dash_queue_ticks - 1 {
            p.dash_input(P, true);
        }
        p.grounded = true;
        p.dash_input(P, true);
        assert_eq!(
            p.dash_left, P.dash_ticks,
            "the buffered press fired on landing"
        );
    }
}

#[cfg(test)]
mod wall_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        wallslide_speed: -8 * ONE,
        wall_sticky_ticks: 4,
        walljump_speed: 16 * ONE,
        walljump_decel: (16 - 8) * ONE / 12,
        wall_lock_short: 6,
        wall_lock_long: 12,
        double_jump_speed: 18 * ONE,
        double_jump_delay_ticks: 4,
        double_jump_ticks: 11,
        double_jump_queue_ticks: 12,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    /// A floor at y=0 and a wall whose face is at x=4.
    const EDGES: [[i32; 4]; 2] = [[-40 * ONE, 0, 40 * ONE, 0], [4 * ONE, 0, 4 * ONE, 40 * ONE]];
    fn room() -> impl Fn(usize) -> [i32; 4] {
        |i| EDGES[i]
    }
    /// Airborne against the wall's face, falling, with the Claw.
    fn against_wall() -> Player {
        let mut p = Player::spawn(3 * ONE, 30 * ONE);
        p.has_walljump = true;
        for _ in 0..8 {
            p.step(P, 1, false, 2, room());
            if p.touching_wall != 0 {
                break;
            }
        }
        assert_eq!(
            p.touching_wall, 1,
            "the probe finds the wall it just clamped against"
        );
        assert!(
            !p.wall_sliding,
            "the slide only starts on the step after contact"
        );
        p
    }
    #[test]
    fn without_the_claw_the_wall_is_just_a_wall() {
        let mut p = against_wall();
        p.has_walljump = false;
        for _ in 0..20 {
            p.step(P, 1, false, 2, room());
            assert!(!p.wall_sliding);
        }
        assert!(p.vy < P.wallslide_speed, "still in free fall");
    }
    #[test]
    fn a_slide_clamps_the_fall_and_gives_back_the_air_abilities() {
        let mut p = against_wall();
        p.air_dashed = true;
        p.double_jumped = true;
        p.step(P, 1, false, 2, room());
        assert!(p.wall_sliding);
        assert!(!p.air_dashed && !p.double_jumped);
        assert_eq!(p.facing, 1, "turned into the wall");
        for _ in 0..20 {
            p.step(P, 1, false, 2, room());
            assert!(
                p.vy >= P.wallslide_speed,
                "the slide is a floor on the fall"
            );
        }
        assert_eq!(p.vy, P.wallslide_speed, "and gravity pins it there");
    }
    #[test]
    fn the_slide_holds_when_the_stick_is_let_go_and_ends_when_it_is_held_away() {
        let mut p = against_wall();
        p.step(P, 1, false, 2, room());
        assert!(p.wall_sliding);
        for _ in 0..10 {
            p.step(P, 0, false, 2, room());
            assert!(
                p.wall_sliding,
                "WALL_STICKY_STEPS only counts the away direction"
            );
        }
        for _ in 0..P.wall_sticky_ticks {
            p.step(P, -1, false, 2, room());
        }
        assert!(!p.wall_sliding);
    }
    #[test]
    fn a_wall_jump_kicks_away_at_its_own_speed_until_the_lock_runs_out() {
        let mut p = against_wall();
        p.step(P, 1, false, 2, room());
        let x = p.x;
        p.step(P, 1, true, 2, room());
        assert!(p.wall_locked && p.jumping && !p.wall_sliding);
        assert_eq!(p.wall_jumped, -1, "kicked away from a wall on the right");
        assert_eq!(p.facing, -1);
        assert!(p.x < x);
        // Neutral stick, so only WJLOCK_STEPS_LONG can end the lock.
        for tick in 0..P.wall_lock_long {
            let before = p.x;
            p.step(P, 0, true, 2, room());
            assert!(
                p.x < before,
                "tick {tick}: the lock owns horizontal velocity"
            );
        }
        assert!(!p.wall_locked);
        let before = p.x;
        p.step(P, 1, true, 2, room());
        assert!(p.x > before, "input is back");
    }
    #[test]
    fn pressing_back_releases_the_lock_once_the_short_window_passes() {
        let mut p = against_wall();
        p.step(P, 1, false, 2, room());
        p.step(P, 1, true, 2, room());
        for _ in 0..P.wall_lock_short - 1 {
            p.step(P, 1, true, 2, room());
            assert!(p.wall_locked);
        }
        p.step(P, 1, true, 2, room());
        assert!(
            !p.wall_locked,
            "holding back past WJLOCK_STEPS_SHORT releases it"
        );
    }
    #[test]
    fn the_double_jump_waits_for_the_wings_and_runs_once_per_airtime() {
        let mut p = Player::spawn(0, 10 * ONE);
        p.has_double_jump = true;
        p.step(P, 0, true, 1, room());
        assert!(p.double_jumping && p.double_jumped);
        for tick in 0..P.double_jump_delay_ticks {
            p.step(P, 0, true, 1, room());
            assert!(p.vy < 0, "tick {tick} is still the flourish");
        }
        p.step(P, 0, true, 1, room());
        // Pre-compensated like `jump`: the source sets velocity, then the
        // physics step applies gravity, so the held value is one tick lower.
        assert_eq!(p.vy, P.double_jump_speed - P.gravity / 60);
        // A release does not cancel it; only the step count does.
        for _ in 0..P.double_jump_ticks {
            p.step(P, 0, false, 1, room());
        }
        assert!(!p.double_jumping);
        p.was_jump = false;
        p.step(P, 0, true, 1, room());
        assert!(!p.double_jumping, "one per airtime");
    }
}

#[cfg(test)]
mod super_dash_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        super_dash_speed: 30 * ONE,
        super_dash_charge_ticks: 48,
        super_dash_cancel_ticks: 12,
        super_dash_recover_ticks: 30,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    const EDGES: [[i32; 4]; 2] = [[-40 * ONE, 0, 40 * ONE, 0], [4 * ONE, 0, 4 * ONE, 40 * ONE]];
    fn room() -> impl Fn(usize) -> [i32; 4] {
        |i| EDGES[i]
    }
    fn charged() -> Player {
        let mut p = Player::spawn(-10 * ONE, ONE);
        p.has_super_dash = true;
        p.grounded = true;
        p.facing = 1;
        for _ in 0..P.super_dash_charge_ticks {
            p.super_dash_input(P, true);
            p.step(P, 0, false, 2, room());
        }
        assert_eq!(p.super_dash, SuperDash::Ready);
        p
    }
    #[test]
    fn without_the_heart_the_button_does_nothing() {
        let mut p = Player::spawn(0, ONE);
        p.grounded = true;
        p.super_dash_input(P, true);
        assert_eq!(p.super_dash, SuperDash::Off);
    }
    #[test]
    fn letting_go_before_the_charge_completes_gives_control_back() {
        let mut p = Player::spawn(0, ONE);
        p.has_super_dash = true;
        p.grounded = true;
        for _ in 0..P.super_dash_charge_ticks - 1 {
            p.super_dash_input(P, true);
            p.step(P, 0, false, 2, room());
        }
        assert!(matches!(p.super_dash, SuperDash::Charging(_)));
        p.super_dash_input(P, false);
        assert_eq!(p.super_dash, SuperDash::Off);
    }
    #[test]
    fn leaving_the_ground_drops_the_charge() {
        let mut p = charged();
        p.super_dash = SuperDash::Charging(4);
        p.grounded = false;
        p.super_dash_input(P, true);
        assert_eq!(
            p.super_dash,
            SuperDash::Off,
            "Ground Charge watches Y Speed"
        );
    }
    #[test]
    fn a_charged_release_travels_at_its_own_speed_until_a_wall() {
        let mut p = charged();
        p.super_dash_input(P, false);
        assert_eq!(p.super_dash, SuperDash::Travelling(0));
        let start = p.x;
        p.step(P, 0, false, 2, room());
        assert_eq!(
            p.x - start,
            P.super_dash_speed / 60,
            "the FSM speed, not RUN_SPEED"
        );
        assert_eq!(p.vy, 0, "SetGravity2dScale zeroes it for the travel");
        for _ in 0..200 {
            p.step(P, 0, false, 2, room());
            if matches!(p.super_dash, SuperDash::Recovering(_)) {
                break;
            }
        }
        assert!(
            matches!(p.super_dash, SuperDash::Recovering(_)),
            "stopped at the wall"
        );
        assert_eq!(p.x, 4 * ONE - P.half_width);
        for _ in 0..P.super_dash_recover_ticks {
            let held = p.x;
            p.step(P, -1, false, 2, room());
            assert_eq!(p.x, held, "no control through the Hit Wall recovery");
        }
        p.step(P, -1, false, 2, room());
        assert_eq!(p.super_dash, SuperDash::Off);
        assert!(p.x < 4 * ONE - P.half_width, "control is back");
    }
    #[test]
    fn a_jump_only_cancels_the_travel_after_the_locked_window() {
        let mut p = charged();
        p.super_dash_input(P, false);
        for _ in 0..P.super_dash_cancel_ticks {
            p.was_jump = false;
            p.step(P, 0, true, 2, room());
            assert!(matches!(p.super_dash, SuperDash::Travelling(_)), "locked");
        }
        p.was_jump = false;
        p.step(P, 0, true, 2, room());
        assert_eq!(p.super_dash, SuperDash::Off);
    }
}

#[cfg(test)]
mod shade_cloak_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        dash_speed: 20 * ONE,
        dash_ticks: 15,
        dash_cooldown_ticks: 36,
        dash_queue_ticks: 10,
        shadow_dash_cooldown_ticks: 90,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    fn flat() -> impl Fn(usize) -> [i32; 4] {
        |_| [0, 0, 0, 0]
    }
    fn hero() -> Player {
        let mut p = Player::spawn(0, 0);
        p.has_dash = true;
        p.has_shade_cloak = true;
        p.grounded = true;
        p
    }
    fn dash(p: &mut Player) {
        p.dash_input(P, false);
        p.dash_input(P, true);
        for _ in 0..P.dash_ticks {
            p.step(P, 0, false, 0, flat());
        }
    }
    #[test]
    fn without_the_cloak_a_dash_is_never_invulnerable() {
        let mut p = hero();
        p.has_shade_cloak = false;
        p.dash_input(P, true);
        assert!(!p.shadow_dashing);
    }
    #[test]
    fn the_cloak_makes_the_dash_invulnerable_for_exactly_its_duration() {
        let mut p = hero();
        p.dash_input(P, true);
        assert!(p.shadow_dashing);
        for _ in 0..P.dash_ticks - 1 {
            p.step(P, 0, false, 0, flat());
            assert!(p.shadow_dashing, "the whole dash carries the i-frames");
        }
        p.step(P, 0, false, 0, flat());
        assert!(!p.shadow_dashing, "and not one tick longer");
    }
    #[test]
    fn its_own_cooldown_outlasts_the_dash_cooldown() {
        let mut p = hero();
        dash(&mut p);
        // The ordinary dash is available again well before the Cloak is.
        for _ in 0..P.dash_cooldown_ticks {
            p.dash_input(P, false);
        }
        // The flat fixture has no floor, so put the Knight back on the ground
        // rather than spending the one air dash CanDash allows.
        p.grounded = true;
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks, "the dash itself is ready");
        assert!(!p.shadow_dashing, "but this one is an ordinary dash");
        for _ in 0..P.shadow_dash_cooldown_ticks {
            p.dash_input(P, false);
            p.step(P, 0, false, 0, flat());
        }
        p.grounded = true;
        p.dash_input(P, true);
        assert!(p.shadow_dashing);
    }
}

#[cfg(test)]
mod priority_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        dash_speed: 20 * ONE,
        dash_ticks: 15,
        dash_cooldown_ticks: 36,
        dash_queue_ticks: 10,
        ledge_buffer_ticks: 2,
        head_bump_ticks: 4,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    // A cooldown longer than the swing, so the queue window is reachable.
    const A: AttackParams = AttackParams {
        duration: 25,
        cooldown: 32,
        alternate_reset: 36,
        hit_start: 1,
        hit_end: 6,
        queue_ticks: 6,
        recovery_ticks: 8,
    };
    /// A floor that stops at x=0, so walking right leaves it.
    const EDGES: [[i32; 4]; 2] = [
        [-40 * ONE, 0, 0, 0],
        [-6 * ONE, 12 * ONE, 6 * ONE, 12 * ONE],
    ];
    fn ledge() -> impl Fn(usize) -> [i32; 4] {
        |i| EDGES[i]
    }
    fn on_floor() -> Player {
        let mut p = Player::spawn(-4 * ONE, ONE);
        p.has_dash = true;
        p.step(P, 0, false, 1, ledge());
        assert!(p.grounded);
        p
    }
    #[test]
    fn the_ledge_buffer_still_allows_a_jump_just_after_walking_off() {
        let mut p = on_floor();
        while p.grounded {
            p.step(P, 1, false, 1, ledge());
        }
        assert!(p.ledge_buffer > 0, "walking off arms the buffer");
        p.step(P, 1, true, 1, ledge());
        assert!(p.jumping, "the source's coyote time");
        assert_eq!(p.ledge_buffer, 0, "and it is spent, not re-offered");
    }
    #[test]
    fn past_the_ledge_buffer_the_jump_is_refused() {
        let mut p = on_floor();
        while p.grounded {
            p.step(P, 1, false, 1, ledge());
        }
        for _ in 0..P.ledge_buffer_ticks {
            p.step(P, 1, false, 1, ledge());
        }
        p.step(P, 1, true, 1, ledge());
        assert!(!p.jumping);
    }
    #[test]
    fn a_ceiling_refuses_the_next_jumps() {
        let mut p = Player::spawn(0, 8 * ONE);
        p.vy = 30 * ONE;
        // Rise into the low ceiling the second edge provides.
        for _ in 0..10 {
            p.step(P, 0, false, 2, ledge());
            if p.head_bump > 0 {
                break;
            }
        }
        assert!(p.head_bump > 0, "the bump is recorded");
        p.grounded = true;
        p.step(P, 0, true, 2, ledge());
        assert!(!p.jumping, "HEAD_BUMP_STEPS refuses it");
    }
    #[test]
    fn a_dash_refuses_a_jump_and_a_swing_and_the_swing_is_kept_queued() {
        let mut p = on_floor();
        let mut nail = Nail::new();
        p.dash_input(P, true);
        assert!(p.dash_left > 0);
        p.step(P, 0, true, 1, ledge());
        assert!(!p.jumping, "CanJump refuses a jump out of a dash");
        assert!(
            !nail.tick(A, true, 0, &mut p),
            "CanAttack refuses a swing out of a dash"
        );
        // ATTACK_QUEUE_STEPS is shorter than a dash, so that press is lost, as
        // it is in the source. The buffer is for the cooldown's tail instead.
        for _ in 0..p.dash_left {
            p.step(P, 0, false, 1, ledge());
            nail.tick(A, true, 0, &mut p);
        }
        assert!(
            !nail.active,
            "the queue is too short to outlast a whole dash"
        );
    }
    #[test]
    fn a_swing_pressed_inside_the_queue_window_lands_when_the_cooldown_ends() {
        let mut p = on_floor();
        let mut nail = Nail::new();
        assert!(nail.tick(A, true, 0, &mut p));
        // Run out the swing and all but a few ticks of its cooldown.
        for _ in 0..A.cooldown - 4 {
            p.step(P, 0, false, 1, ledge());
            nail.tick(A, false, 0, &mut p);
        }
        assert!(!nail.active);
        // Press early: a strict edge would drop this, the queue holds it.
        let mut landed = false;
        for _ in 0..4 {
            p.step(P, 0, false, 1, ledge());
            landed |= nail.tick(A, true, 0, &mut p);
        }
        assert!(landed, "ATTACK_QUEUE_STEPS carried the early press");
    }
    #[test]
    fn an_attack_refuses_a_dash_until_its_recovery_passes() {
        let mut p = on_floor();
        let mut nail = Nail::new();
        assert!(nail.tick(A, true, 0, &mut p));
        assert!(p.attack_recovering);
        p.dash_input(P, true);
        assert_eq!(p.dash_left, 0, "CanDash refuses one inside the recovery");
        for _ in 0..A.recovery_ticks {
            p.step(P, 0, false, 1, ledge());
            nail.tick(A, false, 0, &mut p);
        }
        assert!(!p.attack_recovering);
        p.dash_input(P, false);
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks);
    }
}

/// P11 step 4: the bouncy target. The world side decides what the down slash
/// meets; this is only what the hero does once it has met one.
#[cfg(test)]
mod shroom_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        shroom_speed: 25 * ONE,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    /// Open air: nothing but gravity acts on the rise.
    fn free(p: &mut Player, jump: bool) {
        p.step(P, 0, jump, 0, |_| [0; 4]);
    }
    /// The highest the Knight gets above where it started, over `ticks`.
    fn apex(mut p: Player, jump: bool, ticks: u32) -> i32 {
        let start = p.y;
        let mut top = 0;
        for _ in 0..ticks {
            free(&mut p, jump);
            top = top.max(p.y - start);
        }
        top
    }
    #[test]
    fn the_bounce_replaces_the_fall_and_gives_back_the_air_abilities() {
        let mut p = Player::spawn(0, 20 * ONE);
        p.vy = -12 * ONE;
        p.air_dashed = true;
        p.double_jumped = true;
        p.shroom_bounce(P);
        assert_eq!(
            p.vy, P.shroom_speed,
            "the source sets the velocity outright"
        );
        assert!(p.shroom_bouncing);
        assert!(
            !p.air_dashed && !p.double_jumped,
            "ShroomBounce clears both"
        );
    }
    #[test]
    fn the_flag_lasts_exactly_as_long_as_the_rise() {
        let mut p = Player::spawn(0, 0);
        p.shroom_bounce(P);
        let mut ticks = 0;
        while p.shroom_bouncing {
            free(&mut p, false);
            ticks += 1;
            assert_eq!(p.shroom_bouncing, p.vy > 0, "tick {ticks}");
        }
        // 25 units of rise shed at 47 a second, sampled at 60 Hz.
        assert_eq!(ticks, 32);
    }
    #[test]
    fn a_ceiling_ends_the_flag_with_the_rise() {
        const CEILING: [[i32; 4]; 1] = [[-40 * ONE, 4 * ONE, 40 * ONE, 4 * ONE]];
        let mut p = Player::spawn(0, 0);
        p.shroom_bounce(P);
        for _ in 0..20 {
            p.step(P, 0, false, 1, |i| CEILING[i]);
            if !p.shroom_bouncing {
                break;
            }
        }
        assert_eq!(p.vy, 0, "the head bump stopped the rise short");
        assert!(!p.shroom_bouncing, "and the flag went with it");
        assert_eq!(p.y + P.top, 4 * ONE);
    }
    #[test]
    fn the_impulse_survives_an_unfinished_jump_hold() {
        let mut p = Player::spawn(0, 0);
        p.grounded = true;
        free(&mut p, true);
        assert!(p.jumping, "the hold is still running");
        p.shroom_bounce(P);
        free(&mut p, true);
        assert_eq!(
            p.vy,
            P.shroom_speed - P.gravity / 60,
            "the hold cannot re-pin JUMP_SPEED"
        );
    }
    #[test]
    fn the_shroom_throws_the_knight_higher_than_its_own_jump() {
        let mut held = Player::spawn(0, 0);
        held.grounded = true;
        let mut bounced = Player::spawn(0, 0);
        bounced.shroom_bounce(P);
        assert!(
            apex(bounced, false, 120) > apex(held, true, 120),
            "SHROOM_BOUNCE_VELOCITY beats a fully held JUMP_SPEED"
        );
    }
}

/// P14 step 7: the ability boundaries and combinations, which is where a
/// re-implementation usually diverges from the source even when each ability
/// on its own is right.
#[cfg(test)]
mod combination_tests {
    use super::*;
    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        dash_speed: 20 * ONE,
        dash_ticks: 15,
        dash_cooldown_ticks: 36,
        dash_queue_ticks: 10,
        shadow_dash_cooldown_ticks: 90,
        wallslide_speed: -8 * ONE,
        wall_sticky_ticks: 4,
        walljump_speed: 16 * ONE,
        walljump_decel: (16 - 8) * ONE / 12,
        wall_lock_short: 6,
        wall_lock_long: 12,
        double_jump_speed: 18 * ONE,
        double_jump_delay_ticks: 4,
        double_jump_ticks: 11,
        double_jump_queue_ticks: 12,
        super_dash_speed: 30 * ONE,
        super_dash_charge_ticks: 48,
        super_dash_cancel_ticks: 12,
        super_dash_recover_ticks: 30,
        ledge_buffer_ticks: 2,
        head_bump_ticks: 4,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE,
        ..Params::ZERO
    };
    /// A floor at y=0 and a wall whose face is at x=4.
    const EDGES: [[i32; 4]; 2] = [[-40 * ONE, 0, 40 * ONE, 0], [4 * ONE, 0, 4 * ONE, 40 * ONE]];
    fn room() -> impl Fn(usize) -> [i32; 4] {
        |i| EDGES[i]
    }
    /// Jump off the floor and hold right until the wall slide takes. A jump
    /// only fires on a press made while already grounded, so this settles the
    /// Knight on the floor before pressing.
    fn slide_up_the_wall(p: &mut Player) {
        p.step(P, 0, false, 2, room());
        p.was_jump = false;
        p.step(P, 1, true, 2, room());
        assert!(p.jumping, "left the floor");
        for _ in 0..90 {
            p.step(P, 1, true, 2, room());
            if p.wall_sliding {
                return;
            }
        }
        panic!("never reached the wall");
    }
    fn knight() -> Player {
        let mut p = Player::spawn(0, ONE);
        p.has_dash = true;
        p.has_walljump = true;
        p.has_double_jump = true;
        p.has_super_dash = true;
        p
    }
    #[test]
    fn wall_slide_then_wall_jump_then_dash_all_land() {
        let mut p = knight();
        slide_up_the_wall(&mut p);
        // The slide handed the air dash back, and the wall jump does again.
        p.was_jump = false;
        p.step(P, 1, true, 2, room());
        assert!(p.wall_locked && p.wall_jumped == -1);
        assert!(!p.air_dashed);
        p.dash_input(P, true);
        assert_eq!(
            p.dash_left, P.dash_ticks,
            "a dash out of a wall jump is allowed"
        );
    }
    #[test]
    fn a_dash_into_a_wall_stops_at_it_rather_than_tunnelling() {
        let mut p = knight();
        p.x = 3 * ONE;
        p.facing = 1;
        p.dash_input(P, true);
        for _ in 0..P.dash_ticks {
            p.step(P, 1, false, 2, room());
        }
        assert_eq!(
            p.x,
            4 * ONE - P.half_width,
            "flush with the wall, not through it"
        );
    }
    #[test]
    fn a_spent_air_dash_comes_back_from_a_wall_but_not_from_the_air() {
        let mut p = knight();
        p.grounded = false;
        p.y = 20 * ONE;
        p.dash_input(P, true);
        for _ in 0..P.dash_ticks + P.dash_cooldown_ticks {
            p.dash_input(P, false);
            p.step(P, 0, false, 2, room());
        }
        p.dash_input(P, true);
        assert_eq!(p.dash_left, 0, "airDashed is still spent");
        // CanDash allows one off a wall even with the air dash gone.
        p.wall_sliding = true;
        p.dash_input(P, false);
        p.dash_input(P, true);
        assert_eq!(p.dash_left, P.dash_ticks);
    }
    #[test]
    fn mashing_a_button_never_beats_its_cooldown() {
        let mut p = knight();
        p.dash_input(P, true);
        let mut dashes = 1;
        for tick in 0..P.dash_cooldown_ticks * 3 {
            // Alternate every tick, the worst case a player can produce.
            p.dash_input(P, tick % 2 == 0);
            if p.dash_left == P.dash_ticks {
                dashes += 1;
            }
            p.step(P, 0, false, 2, room());
            p.grounded = true;
        }
        // Three cooldowns can yield at most three more dashes, never one a tick.
        assert!(dashes <= 4, "{dashes} dashes in three cooldowns");
    }
    #[test]
    fn a_double_jump_is_still_available_after_a_wall_jump() {
        let mut p = knight();
        slide_up_the_wall(&mut p);
        p.was_jump = false;
        p.step(P, 1, true, 2, room());
        assert!(p.wall_locked, "wall jumped");
        assert!(!p.double_jumped, "the wall jump handed the wings back");
        // Let the lock run out, then press again in mid-air.
        for _ in 0..P.wall_lock_long + 1 {
            p.step(P, 0, true, 2, room());
        }
        p.was_jump = false;
        p.step(P, 0, true, 2, room());
        assert!(p.double_jumping);
    }
}
