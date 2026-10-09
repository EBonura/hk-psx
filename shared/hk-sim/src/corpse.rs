//! Source corpse lifecycle using the port's bounded terrain solver.
//! Source Corpse retains its final sprite after Land/one-second Complete(false,
//! false). Steam/spatter and the full Box2D contact/friction solver are separate.
use crate::{Params, Player, ONE};
#[derive(Clone, Copy, Debug)]
pub struct CorpseSpec {
    pub air_clip: u16,
    pub land_clip: u16,
    pub bounds: [i32; 4],
    pub spawn_offset: [i32; 2],
    /// Source ObjectBounce factor in Q16; validated in [0, 1].
    pub bounce_factor: i32,
    /// Source EnemyDeathEffects.corpseFlingSpeed in Q16 (15 for the Crawler
    /// and Runner corpses, 20 for the Climber); the launch table is scaled.
    pub fling_speed: i32,
    /// Rigidbody2D gravityScale * 60 in Q16 (48 for the 0.8 Crawler/Runner/
    /// Climber corpses, 42 for the 0.7 Buzzer corpse).
    pub gravity: i32,
    /// Source Corpse.breaker: Land counts a bounce and smashes once the count
    /// reaches `smash_bounces` (0 smashes on the first landing). Break pieces
    /// are not presented.
    pub breaker: bool,
    pub smash_bounces: u8,
    /// Ticks after landing until the corpse is removed (0 keeps it): the
    /// source Roller corpse rolls out, shrinks and destroys itself.
    pub remove_after_land: u16,
    /// Non-zero selects the corpse prefab without a Rigidbody2D: it never
    /// launches or falls, holds `air_clip` for this many ticks and then plays
    /// `land_clip` until `remove_after_land`. Zero keeps the flung body above.
    pub hold_ticks: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpsePhase {
    Air,
    Land,
    Rest,
    Removed,
}
#[derive(Clone, Copy, Debug)]
pub struct Corpse {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub facing: i32,
    pub phase: CorpsePhase,
    pub animation_tick: u32,
    land_ticks: u16,
    grounded: bool,
    bounces: u8,
    rng: u32,
}
// Source up-hit angle is uniform75..105 degrees, speed15*1.3. Quantized to
// 31 original-range directions; this deterministic sequence is not Unity RNG.
const UP: [[i32; 2]; 31] = [
    [330758, 1234407],
    [309165, 1239991],
    [287477, 1245198],
    [265701, 1250026],
    [243845, 1254472],
    [221914, 1258537],
    [199916, 1262218],
    [177857, 1265515],
    [155743, 1268426],
    [133582, 1270951],
    [111381, 1273089],
    [89145, 1274839],
    [66883, 1276201],
    [44600, 1277174],
    [22303, 1277757],
    [0, 1277952],
    [-22303, 1277757],
    [-44600, 1277174],
    [-66883, 1276201],
    [-89145, 1274839],
    [-111381, 1273089],
    [-133582, 1270951],
    [-155743, 1268426],
    [-177857, 1265515],
    [-199916, 1262218],
    [-221914, 1258537],
    [-243845, 1254472],
    [-265701, 1250026],
    [-287477, 1245198],
    [-309165, 1239991],
    [-330758, 1234407],
];
impl Corpse {
    pub fn spawn(spec: CorpseSpec, x: i32, y: i32, kind: u16, facing: i32, seed: u32) -> Self {
        assert!(
            (0..=ONE).contains(&spec.bounce_factor),
            "invalid corpse bounce factor"
        );
        let mut corpse = Self {
            x: x + spec.spawn_offset[0],
            y: y + spec.spawn_offset[1],
            vx: 0,
            vy: 0,
            facing,
            phase: CorpsePhase::Air,
            animation_tick: 0,
            land_ticks: 0,
            grounded: false,
            bounces: 0,
            rng: seed,
        };
        if spec.hold_ticks != 0 {
            return corpse; // No Rigidbody2D on this prefab: nothing launches it.
        }
        assert!(
            (ONE..=64 * ONE).contains(&spec.fling_speed),
            "invalid corpse fling speed"
        );
        let v = match kind {
            2 => UP[(corpse.random() % 31) as usize],
            3 => [0, -15 * ONE],
            _ => [facing * 491520, 851338], // cos/sin60deg *15, rounded Q16.
        };
        // The table is authored for the common 15 units/s launch.
        let scale = |q: i32| psx_math::int32::mul_div_i32(q, spec.fling_speed, 15 * ONE);
        corpse.vx = scale(v[0]);
        corpse.vy = scale(v[1]);
        corpse
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng
    }
    pub fn clip(self, spec: CorpseSpec) -> u16 {
        if self.phase == CorpsePhase::Air {
            spec.air_clip
        } else {
            spec.land_clip
        }
    }
    pub fn visible(self) -> bool {
        self.phase != CorpsePhase::Removed
    }
    /// Source `Corpse Egg Sac` Control FSM without its physics: Spit plays the
    /// first clip and waits, Burst plays the second to completion and End
    /// deactivates the object. With no body there is no terrain to solve
    /// against, so the corpse rests on the spawn point for its whole life.
    fn hold(&mut self, spec: CorpseSpec) {
        self.animation_tick = self.animation_tick.saturating_add(1);
        if self.phase == CorpsePhase::Air {
            if self.animation_tick >= spec.hold_ticks as u32 {
                self.phase = CorpsePhase::Land;
                self.animation_tick = 0;
            }
        } else if spec.remove_after_land != 0
            && self.animation_tick >= spec.remove_after_land as u32
        {
            self.phase = CorpsePhase::Removed;
        }
    }
    /// Terrain-apron residency is checked by the caller. No extra allocation,
    /// timer-based corpse deletion, contact damage, or Geo gameplay is added.
    pub fn tick(&mut self, spec: CorpseSpec, count: usize, edge: impl Fn(usize) -> [i32; 4]) {
        if self.phase == CorpsePhase::Removed {
            return;
        }
        if spec.hold_ticks != 0 {
            return self.hold(spec);
        }
        self.animation_tick = self.animation_tick.saturating_add(1);
        if matches!(self.phase, CorpsePhase::Land | CorpsePhase::Rest) {
            self.land_ticks = self.land_ticks.saturating_add(1);
            if self.phase == CorpsePhase::Land && self.land_ticks >= 60 {
                self.phase = CorpsePhase::Rest;
            }
            if spec.remove_after_land != 0 && self.land_ticks >= spec.remove_after_land {
                self.phase = CorpsePhase::Removed;
                return;
            }
        }
        let b = if self.facing < 0 {
            spec.bounds
        } else {
            [
                -spec.bounds[2],
                spec.bounds[1],
                -spec.bounds[0],
                spec.bounds[3],
            ]
        };
        let offset = b[0] + (b[2] - b[0]) / 2;
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        let params = Params {
            speed: self.vx.abs(),
            gravity: spec.gravity,
            fall: 100 * ONE,
            half_width: (b[2] - b[0]) / 2,
            bottom: b[1],
            top: b[3],
            ..Params::ZERO
        };
        let old_x = body.x;
        let old_vy = self.vy;
        body.step(params, self.vx.signum(), false, count, edge);
        let hit_wall = body.x != old_x + self.vx / 60;
        let hit_floor = body.grounded && old_vy < 0;
        let hit_ceiling = !body.grounded && old_vy > 0 && body.vy == 0;
        self.x = body.x - offset;
        self.y = body.y;
        self.vy = body.vy;
        self.grounded = body.grounded;
        if hit_floor && self.phase == CorpsePhase::Air {
            if spec.breaker {
                self.bounces = self.bounces.saturating_add(1);
                if self.bounces >= spec.smash_bounces {
                    self.phase = CorpsePhase::Removed;
                    return;
                }
                // Still airborne after the bounce below; Land never plays.
            } else {
                self.phase = CorpsePhase::Land;
                self.animation_tick = 0;
                self.land_ticks = 0;
            }
        }
        // ObjectBounce reflects contact velocity, source factor * Random(0.8,1.2).
        // Axis contacts reuse the proven body solver. Slope response/friction is
        // a bounded approximation to Unity's full contact manifold.
        let speed2 = self.vx as i64 * self.vx as i64 + old_vy as i64 * old_vy as i64;
        if (hit_floor || hit_wall || hit_ceiling) && speed2 > (ONE as i64 * ONE as i64) {
            assert!(
                (0..=ONE).contains(&spec.bounce_factor),
                "invalid corpse bounce factor"
            );
            // Rounded Q16 endpoints preserve the existing Crawler's exact
            // [15729,23593] distribution and RNG sequence at factor19661.
            let low = (spec.bounce_factor * 4 + 2) / 5;
            let high = (spec.bounce_factor * 6 + 2) / 5;
            let factor = low + (self.random() % (high - low + 1) as u32) as i32;
            self.vx = ((self.vx as i64 * factor as i64) >> 16) as i32;
            self.vy = ((old_vy as i64 * factor as i64) >> 16) as i32;
            if hit_wall {
                self.vx = -self.vx;
            }
            if hit_floor || hit_ceiling {
                self.vy = -self.vy;
                self.grounded = false;
            }
        } else if body.grounded {
            // Source corpse PhysicsMaterial2D 'Geo Small' friction0.2. Coulomb
            // step uses that coefficient; arbitrary terrain material mixing is
            // not represented by the cooked edge format.
            // friction .2 * g / 60. The quotient fits i32 (so matches the i64
            // form) for |gravity| below 2^31 * 60 / 13107, about 150 units/s^2
            // in Q16; cooked corpses use 42 and 48.
            self.vx = self.vx.signum()
                * (self.vx.abs() - (psx_math::int32::mul_div_i32(spec.gravity, 13107, 60) >> 16))
                    .max(0);
        }
        // Source Corpse.Update only destroys an airborne corpse below y=-10.
        if self.phase == CorpsePhase::Air && self.y < -10 * ONE {
            self.phase = CorpsePhase::Removed;
        }
    }
}
