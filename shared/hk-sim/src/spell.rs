//! Vengeful Spirit, from the Hero's `Spell Control` FSM and the projectile's
//! own `Fireball Control` and `damages_enemy` pair.
//!
//! The cast and the projectile are separate objects in the source, and they
//! stay separate here: `Cast` runs on the Knight and pays the SOUL, `Fireball`
//! flies on its own and carries the damage. One is in flight at a time, which
//! is what the source's single pooled object gives in practice.
use crate::Player;

#[derive(Clone, Copy)]
pub struct FireballParams {
    /// `Button Down Time`: the cast button released within this casts, held
    /// past it focuses instead.
    pub tap_ticks: u16,
    /// `MP Cost`, paid on the cast and refunded if the cast never happens.
    pub cost: u16,
    pub speed: i32,
    /// The projectile's own Idle wait before it dissipates on its own.
    pub life_ticks: u16,
    pub damage: u16,
    /// Projectile box relative to its own origin, facing right.
    pub bounds: [i32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastPhase {
    Off,
    /// `Fireball Antic`, during which the Knight is pinned.
    Antic,
    /// `Fireball Recoil`: the cast clip plays out while the Knight is pushed
    /// back with gravity off. The projectile has already left by then, because
    /// `Fireball 1` spawns it and sends FINISHED in the same frame.
    Recoil,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fireball {
    pub alive: bool,
    pub x: i32,
    pub y: i32,
    pub facing: i32,
    pub life: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cast {
    pub phase: CastPhase,
    pub tick: u16,
    pub ball: Fireball,
    /// Set when Vengeful Spirit has been acquired; nothing grants it yet.
    pub has_fireball: bool,
    was_button: bool,
    held: u16,
}
impl Default for Cast {
    fn default() -> Self {
        Self::new()
    }
}
impl Cast {
    pub const fn new() -> Self {
        Self {
            phase: CastPhase::Off,
            tick: 0,
            ball: Fireball {
                alive: false,
                x: 0,
                y: 0,
                facing: 1,
                life: 0,
            },
            has_fireball: false,
            was_button: false,
            held: 0,
        }
    }
    /// The Knight is pinned for the whole cast, as Spell Control's velocity
    /// writes and the Focus lock already do.
    pub fn locks_control(&self) -> bool {
        self.phase != CastPhase::Off
    }
    /// True on the tick the button's release should start a Focus instead of a
    /// cast, so the caller can keep the source's single-button priority.
    pub fn held_past_tap(&self, p: FireballParams) -> bool {
        self.held > p.tap_ticks
    }
    /// One tick of the cast. `antic_ticks` and `cast_ticks` are the two clip
    /// lengths; `soul` is the caller's SOUL, debited only when a cast starts.
    /// Returns true on the tick the projectile is launched.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        p: FireballParams,
        antic_ticks: u16,
        cast_ticks: u16,
        button: bool,
        soul: &mut u16,
        player: &Player,
        blocked: bool,
    ) -> bool {
        self.fly(p);
        if button {
            self.held = self.held.saturating_add(1);
        }
        let released = !button && self.was_button;
        let tapped = released && self.held <= p.tap_ticks;
        self.was_button = button;
        if !button {
            self.held = 0;
        }
        if self.phase == CastPhase::Off {
            // Can Cast?: HeroController.CanCast, MPCharge >= MP Cost, and
            // Has Fireball? wants fireballLevel above zero.
            let can = self.has_fireball
                && !blocked
                && *soul >= p.cost
                && player.dash_left == 0
                && !player.attack_recovering;
            if tapped && can {
                *soul -= p.cost;
                self.phase = CastPhase::Antic;
                self.tick = 0;
            }
            return false;
        }
        self.tick += 1;
        let (length, next) = match self.phase {
            CastPhase::Antic => (antic_ticks, CastPhase::Recoil),
            CastPhase::Recoil => (cast_ticks, CastPhase::Off),
            CastPhase::Off => unreachable!(),
        };
        if self.tick < length {
            return false;
        }
        let launching = self.phase == CastPhase::Antic;
        self.phase = next;
        self.tick = 0;
        if launching {
            // One pooled object in the source, so a new cast replaces the old.
            self.ball = Fireball {
                alive: true,
                x: player.x,
                y: player.y,
                facing: player.facing,
                life: p.life_ticks,
            };
        }
        launching
    }
    /// Advance the projectile and stop it at a wall, as its Idle state does
    /// through the terrain layer collision events.
    fn fly(&mut self, p: FireballParams) {
        if !self.ball.alive {
            return;
        }
        if self.ball.life == 0 {
            self.ball.alive = false;
            return;
        }
        self.ball.life -= 1;
        self.ball.x += self.ball.facing * p.speed / 60;
    }
    /// The projectile's world box, or none when nothing is in flight.
    pub fn ball_bounds(&self, p: FireballParams) -> Option<[i32; 4]> {
        if !self.ball.alive {
            return None;
        }
        let [x0, y0, x1, y1] = p.bounds;
        // The box is authored facing right; a left-flying ball mirrors it.
        let (left, right) = if self.ball.facing >= 0 {
            (x0, x1)
        } else {
            (-x1, -x0)
        };
        Some([
            self.ball.x + left,
            self.ball.y + y0,
            self.ball.x + right,
            self.ball.y + y1,
        ])
    }
    /// Stop the projectile, as a wall or an enemy contact does.
    pub fn extinguish(&mut self) {
        self.ball.alive = false;
    }
    /// The projectile's Idle state ends on a terrain-layer contact, so a ball
    /// overlapping a vertical edge stops there rather than passing through.
    /// Enemies do not stop it: `Fireball Control` only listens for the wall.
    pub fn stop_at_wall(
        &mut self,
        p: FireballParams,
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) {
        let Some(b) = self.ball_bounds(p) else { return };
        for i in 0..count {
            let [x0, y0, x1, y1] = edge(i);
            if x0 != x1 || y0 == y1 {
                continue;
            }
            if b[3] <= y0.min(y1) || b[1] >= y0.max(y1) {
                continue;
            }
            if b[0] <= x0 && b[2] >= x0 {
                self.ball.alive = false;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;
    const P: FireballParams = FireballParams {
        tap_ticks: 15,
        cost: 33,
        speed: 40 * ONE,
        life_ticks: 27,
        damage: 15,
        bounds: [-50538, -70792, 120586, 68444],
    };
    const ANTIC: u16 = 8;
    const CAST: u16 = 8;
    fn knight() -> Player {
        let mut p = Player::spawn(0, 0);
        p.grounded = true;
        p
    }
    /// Tap: pressed for `ticks`, then released.
    fn tap(c: &mut Cast, soul: &mut u16, p: &Player, ticks: u16) {
        for _ in 0..ticks {
            c.tick(P, ANTIC, CAST, true, soul, p, false);
        }
        c.tick(P, ANTIC, CAST, false, soul, p, false);
    }
    #[test]
    fn without_the_spell_a_tap_costs_nothing() {
        let (mut c, k) = (Cast::new(), knight());
        let mut soul = 99;
        tap(&mut c, &mut soul, &k, 3);
        assert_eq!(soul, 99);
        assert_eq!(c.phase, CastPhase::Off);
    }
    #[test]
    fn a_tap_casts_and_a_hold_does_not() {
        let (mut c, k) = (Cast::new(), knight());
        c.has_fireball = true;
        let mut soul = 99;
        tap(&mut c, &mut soul, &k, P.tap_ticks + 4);
        assert_eq!(
            c.phase,
            CastPhase::Off,
            "held past Button Down Time, so this is a Focus"
        );
        assert_eq!(soul, 99);
        tap(&mut c, &mut soul, &k, 3);
        assert_eq!(c.phase, CastPhase::Antic);
        assert_eq!(soul, 99 - P.cost, "MP Cost is paid at the cast");
    }
    #[test]
    fn too_little_soul_refuses_the_cast_outright() {
        let (mut c, k) = (Cast::new(), knight());
        c.has_fireball = true;
        let mut soul = P.cost - 1;
        tap(&mut c, &mut soul, &k, 3);
        assert_eq!(c.phase, CastPhase::Off);
        assert_eq!(soul, P.cost - 1, "and nothing is taken");
    }
    #[test]
    fn the_projectile_leaves_at_the_end_of_the_cast_clip_and_flies_its_own_life() {
        let (mut c, k) = (Cast::new(), knight());
        c.has_fireball = true;
        let mut soul = 99;
        tap(&mut c, &mut soul, &k, 3);
        let mut launched = false;
        for _ in 0..ANTIC {
            launched |= c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        }
        assert!(launched, "the ball leaves as the cast clip starts");
        assert_eq!(c.phase, CastPhase::Recoil);
        assert!(c.ball.alive);
        let start = c.ball.x;
        c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        assert_eq!(
            c.ball.x - start,
            P.speed / 60,
            "Fire Speed along the facing"
        );
        for _ in 0..P.life_ticks {
            c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        }
        assert!(!c.ball.alive, "it dissipates on its own Idle wait");
    }
    #[test]
    fn a_left_facing_ball_mirrors_its_box() {
        let (mut c, mut k) = (Cast::new(), knight());
        c.has_fireball = true;
        k.facing = -1;
        let mut soul = 99;
        tap(&mut c, &mut soul, &k, 3);
        for _ in 0..ANTIC + 1 {
            c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        }
        let b = c.ball_bounds(P).unwrap();
        assert_eq!(b[0], c.ball.x - P.bounds[2]);
        assert_eq!(b[2], c.ball.x - P.bounds[0]);
        assert!(c.ball.x < 0, "and it flies left");
    }
}
