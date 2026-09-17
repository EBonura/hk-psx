//! Bounded deterministic chamber movement. Q16.16 world units, 60 Hz steps.
#![no_std]
use hk_format::Room;
mod combat;
mod actors;
pub mod runner;
pub mod runner_senses;
pub mod climber;
pub mod vengefly;
mod corpse;
pub use corpse::{Corpse,CorpseSpec,CorpsePhase};
mod vitals;
mod focus;
pub use focus::{Focus,FocusParams,FocusInput,FocusClip,FocusEvents};
mod nail_response;
pub mod persistent;
pub use nail_response::{NailResponse, NailResponseParams};
pub use actors::{resolve_actor_spawn, walker_senses, ActorController, ActorHealth, ActorSpec, EnemyParams, Hit, WalkParams, WalkState, MAX_ACTORS};
pub use vitals::{Hurt, VitalParams, Vitals};
pub use combat::polygon_hits_box;
pub use combat::{AttackParams, Grass, Nail};
pub const ONE: i32 = 65536;
#[derive(Clone, Copy)]
pub struct Params {
    pub speed: i32,
    pub jump: i32,
    pub gravity: i32,
    pub fall: i32,
    pub hold_ticks: u16,
    pub min_ticks: u16,
    pub half_width: i32,
    pub bottom: i32,
    pub top: i32,
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
            animation: 0,
            animation_tick: 0,
            land_tick: 0,
        }
    }
    pub fn tick(&mut self, p: Params, dir: i32, jump: bool, room: &Room) {
        self.step(p, dir, jump, room.counts[5], |i| room.edge(i));
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
        if jump && !self.was_jump && self.grounded {
            self.jumping = true;
            self.jump_tick = 0;
            self.grounded = false;
            self.land_tick = 0;
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
        } else if !jump && self.was_jump && self.vy > 0 {
            self.vy = 0;
        }
        self.was_jump = jump;
        self.vy = (self.vy - p.gravity / 60).max(-p.fall);
        let ox = self.x;
        let oy = self.y;
        let mut nx = ox + dir * p.speed / 60;
        // Axis-separated swept AABB against source terrain edges, with open endpoints.
        // No invented floor: collision comes only from cooked source edges.
        for i in 0..count {
            let [x0, y0, x1, y1] = edge(i);
            if (x0 == x1 && y0 == y1) || x0 != x1 || oy + p.top <= y0.min(y1) + 32 || oy + p.bottom >= y0.max(y1) - 32 {
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
            let height = |x: i32| y0 + (((x.clamp(x0.min(x1), x0.max(x1)) - x0) as i64
                * (y1 - y0) as i64) / (x1 - x0) as i64) as i32;
            let old_surface = height(ox);
            let surface = height(nx);
            let follows_slope = was_ground && !self.jumping && y0 != y1
                && (oy + p.bottom - old_surface).abs() <= ONE / 20;
            if (ny < oy || follows_slope) && oy + p.bottom >= old_surface - ONE / 20
                && (ny + p.bottom <= surface || follows_slope) {
                ny = surface - p.bottom;
                self.vy = 0;
                self.grounded = true;
                self.jumping = false;
            }
            if ny > oy && oy + p.top <= old_surface && ny + p.top >= surface {
                ny = surface - p.top;
                self.vy = 0;
                self.jumping = false;
            }
        }
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
    #[test]
    fn authored_ramp_can_be_walked_up_and_down() {
        let p = params();
        let mut a = Player::spawn(ONE, 2 * ONE);
        a.grounded = true;
        let ramp = [0, 0, 10 * ONE, 10 * ONE];
        for _ in 0..30 { a.step(p, 1, false, 1, |_| ramp); }
        assert!(a.grounded);
        assert!((a.y - a.x - ONE).abs() < 64);
        for _ in 0..30 { a.step(p, -1, false, 1, |_| ramp); }
        assert!(a.grounded);
        assert!((a.x - ONE).abs() < 64);
        assert!((a.y - 2 * ONE).abs() < 64);
    }
}
