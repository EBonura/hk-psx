//! The Hero's spells, from the `Spell Control` FSM: Vengeful Spirit (with the
//! projectile's own `Fireball Control` and `damages_enemy` pair), Desolate Dive
//! and Howling Wraiths.
//!
//! The cast and the projectile are separate objects in the source, and they
//! stay separate here: `Cast` runs on the Knight and pays the SOUL, `Fireball`
//! flies on its own and carries the damage. One is in flight at a time, which
//! is what the source's single pooled object gives in practice.
//!
//! `Cast` also makes `Spell Choice`'s pick, which hands Dive and Wraiths to the
//! Knight (`HeroSpell`): both own his body and velocity until `Spell End`, and
//! their hits are the `Q Fall Damage`, `Q Slam` and `Scr Heads` children of his
//! `Spells` folder, switched on by the phase (`Player::spell_hits`).
use crate::hero_attack::{Class, HeroAttacks, HeroHit, Source};
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
    /// `quakeLevel` and `screamLevel` above zero.
    pub has_quake: bool,
    pub has_scream: bool,
    /// The stick this tick, 1 up and -1 down: `ListenForUp`/`ListenForDown`.
    pub aim: i8,
    /// `Button Down` reads the stick once, on entry (`Pressed Up`, `Pressed Down`).
    press_aim: i8,
    /// Dive or Wraiths picked this tick, for the caller to start on the Knight.
    chosen: Option<Choice>,
    was_button: bool,
    was_quick: bool,
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
            has_quake: false,
            has_scream: false,
            aim: 0,
            press_aim: 0,
            chosen: None,
            was_button: false,
            was_quick: false,
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
        if button && !self.was_button {
            self.press_aim = self.aim;
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
            if tapped && self.can_cast(p, *soul, player, blocked) {
                self.begin(p, soul, self.press_aim);
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
    /// Can Cast?: `HeroController.CanCast` and `MPCharge >= MP Cost` (one `MP Cost`
    /// for every spell). Which spell, if any, is `Spell Choice`'s.
    fn can_cast(&self, p: FireballParams, soul: u16, player: &Player, blocked: bool) -> bool {
        !blocked
            && soul >= p.cost
            && player.dash_left == 0
            && !player.attack_recovering
            && !player.casting()
    }
    /// `Spell Choice`: up asks `Has Scream?`, down `Has Quake?`, and either one
    /// not owned falls through to `Has Fireball?`, which wants `fireballLevel`.
    fn choose(&self, aim: i8) -> Option<Choice> {
        match aim {
            1 if self.has_scream => Some(Choice::Scream),
            -1 if self.has_quake => Some(Choice::Quake),
            _ if self.has_fireball => Some(Choice::Fireball),
            _ => None,
        }
    }
    /// Start the chosen spell. The fireball pays here, as it always has; Dive
    /// and Wraiths pay where their states call `TakeMP` (`HeroSpell` cues).
    fn begin(&mut self, p: FireballParams, soul: &mut u16, aim: i8) -> bool {
        match self.choose(aim) {
            Some(Choice::Fireball) => {
                *soul -= p.cost;
                self.phase = CastPhase::Antic;
                self.tick = 0;
            }
            Some(other) => self.chosen = Some(other),
            None => return false,
        }
        true
    }
    /// Dive or Wraiths, picked this tick: the caller starts it on the Knight.
    pub fn take_chosen(&mut self) -> Option<Choice> {
        self.chosen.take()
    }
    /// The Quick Cast button: a press casts at once, with no Button Down Time
    /// to wait out and so no Focus to start. Call it after `tick` on the same
    /// tick, so a cast it starts runs its first clip tick on the next one, as a
    /// released tap does. Returns true on the tick the cast starts.
    pub fn quick(
        &mut self,
        p: FireballParams,
        button: bool,
        soul: &mut u16,
        player: &Player,
        blocked: bool,
    ) -> bool {
        let pressed = button && !self.was_quick;
        self.was_quick = button;
        // `QC` reads the stick as the press lands.
        pressed
            && self.phase == CastPhase::Off
            && self.can_cast(p, *soul, player, blocked)
            && self.begin(p, soul, self.aim)
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

/// What `Spell Choice` picked: the fireball stays on `Cast`, the other two go
/// to the Knight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Fireball,
    Quake,
    Scream,
}

/// Desolate Dive (level 1), cooked from `Spell Control`'s Quake states and the
/// Knight's `Q Fall Damage` and `Q Slam` (host/spells.py).
#[derive(Clone, Copy)]
pub struct QuakeParams {
    /// `Quake Antic`'s clip.
    pub antic_ticks: u16,
    /// `Q On Ground`'s `Quake Antic Speed` held every frame, scaled by the
    /// antic's `DecelerateV2` every physics step: the rise off the ground.
    pub antic_rise: i32,
    /// `Quake1 Down`'s `SetVelocity2d` y, every frame, until the floor.
    pub fall_speed: i32,
    /// `Quake1 Land`: its clip or its Wait, whichever ends first.
    pub land_ticks: u16,
    /// `INVUL_TIME_QUAKE`, which `Quake Finish`'s `QuakeInvuln` starts.
    pub invuln_ticks: u16,
    /// `Q Fall Damage`, on from `Quake1 Down` to the landing.
    pub fall_damage: u16,
    pub fall_bounds: [i32; 4],
    /// `Q Slam`'s `Hit L` and `Hit R`, switched on by `Hit Box Control` between
    /// the first two triggers of `Quake Blast` (ticks after the landing).
    pub slam_damage: u16,
    pub slam_from: u16,
    pub slam_to: u16,
    /// `Q Slam` in Knight-local units; its hits are world-fixed (`Neg Scale`).
    pub slam_at: [i32; 2],
    /// `Hit L` with its parents' scale applied; `Hit R` is its mirror.
    pub slam: &'static [[i32; 2]],
}

/// Howling Wraiths (level 1), from `Scream Antic1` to `Scream End` and the
/// Knight's `Scr Heads`.
#[derive(Clone, Copy)]
pub struct ScreamParams {
    /// `Scream Start`'s clip.
    pub antic_ticks: u16,
    /// `Scream Burst 1`'s Wait, `End Roar`'s and the `Scream End` clip.
    pub cast_ticks: u16,
    /// `Scr Heads`' hits, on between the first two triggers of `Scream1`
    /// (ticks after the burst).
    pub hit_from: u16,
    pub hit_to: u16,
    pub damage: u16,
    pub at: [i32; 2],
    /// `Hit L` (its mirror is `Hit R`) and `Hit U`.
    pub sides: &'static [[i32; 2]],
    pub up: &'static [[i32; 2]],
}

/// The Knight inside Dive or Wraiths: `RelinquishControl` and gravity off
/// until `Spell End`. Each counts the ticks spent in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroSpell {
    Off,
    QuakeAntic(u16),
    /// Falling at `fall_speed` until the floor.
    QuakeDown,
    QuakeLand(u16),
    /// From `Scream Antic1` (tick 0) through the burst to `Scream End`.
    Scream(u16),
}

/// The edges a caller presents: where `Spell Control` calls `TakeMP` and shakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpellCue {
    None,
    /// `Level Check 2`: the antic is over, MP is paid and the fall begins.
    Fall,
    /// `Scream Burst 1`: MP is paid and the heads go out.
    Burst,
}

impl Player {
    /// A Dive or Wraiths owns the Knight.
    pub fn casting(&self) -> bool {
        !matches!(self.spell, HeroSpell::Off)
    }
    /// `TakeDamage` returns for contact damage (hazardType 1) during the dive
    /// (`SetDamageModeFSM(1)`, HAZARD_ONLY, from `Quake1 Down` to `Quake Finish`)
    /// and while `parryInvulnTimer` runs, as it does for the Shade Cloak's dash.
    pub fn ignores_contact(&self) -> bool {
        self.shadow_dashing
            || self.quake_invuln > 0
            || matches!(self.spell, HeroSpell::QuakeDown | HeroSpell::QuakeLand(_))
    }
    /// `RelinquishControl`'s `ResetMotion` and gravity off.
    fn relinquish(&mut self) {
        self.take_hit();
        self.hard_land = 0;
        self.fall_ticks = 0;
    }
    /// `Quake Antic`: `CancelParryInvuln`, then the rise (`Q On Ground`) or a
    /// hover (`Q Off Ground`).
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn start_quake(&mut self, q: &QuakeParams) {
        let rise = if self.grounded { q.antic_rise } else { 0 };
        self.relinquish();
        self.quake_invuln = 0;
        self.vy = rise;
        self.spell = HeroSpell::QuakeAntic(0);
    }
    /// `Scream Antic1`: still, in the air or not.
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn start_scream(&mut self) {
        self.relinquish();
        self.spell = HeroSpell::Scream(0);
    }
    /// One tick of the spell, before `step`. `Quake1 Down` ends in `step`, on
    /// the floor; everything else ends on its own clock.
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn spell_tick(&mut self, q: &QuakeParams, s: &ScreamParams) -> SpellCue {
        self.quake_invuln = self.quake_invuln.saturating_sub(1);
        let mut cue = SpellCue::None;
        self.spell = match self.spell {
            HeroSpell::QuakeAntic(t) if t + 1 >= q.antic_ticks => {
                cue = SpellCue::Fall;
                self.vy = -q.fall_speed;
                HeroSpell::QuakeDown
            }
            HeroSpell::QuakeAntic(t) => HeroSpell::QuakeAntic(t + 1),
            HeroSpell::QuakeLand(t) if t + 1 >= q.land_ticks => {
                self.quake_invuln = q.invuln_ticks;
                HeroSpell::Off
            }
            HeroSpell::QuakeLand(t) => HeroSpell::QuakeLand(t + 1),
            HeroSpell::Scream(t) if t + 1 >= s.antic_ticks + s.cast_ticks => HeroSpell::Off,
            HeroSpell::Scream(t) => {
                if t + 1 == s.antic_ticks {
                    cue = SpellCue::Burst;
                }
                HeroSpell::Scream(t + 1)
            }
            other => other,
        };
        cue
    }
    /// The spell's live hits, in world space, onto the tick's attack list.
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn spell_hits(&self, q: &QuakeParams, s: &ScreamParams, attacks: &mut HeroAttacks) {
        // (source, damage, parent, Hit L's polygon, Hit U's or none). Hit L faces
        // left (`damages_enemy` direction 180), Hit R is its mirror (0), Hit U
        // points up (90, the up slash's cardinal).
        let (source, damage, parent, sides, up) = match self.spell {
            HeroSpell::QuakeDown => {
                let b = q.fall_bounds;
                let (x, y) = (self.x, self.y);
                let fall = [x + b[0], y + b[1], x + b[2], y + b[3]];
                attacks.push(HeroHit::spell_box(Source::Quake, fall, q.fall_damage, -1));
                return;
            }
            HeroSpell::QuakeLand(t) if (q.slam_from..q.slam_to).contains(&t) => {
                (Source::Quake, q.slam_damage, q.slam_at, q.slam, None)
            }
            HeroSpell::Scream(t)
                if t >= s.antic_ticks && (s.hit_from..s.hit_to).contains(&(t - s.antic_ticks)) =>
            {
                (Source::Scream, s.damage, s.at, s.sides, Some(s.up))
            }
            _ => return,
        };
        // Knight-local x runs against the facing (the collection faces left).
        let at = [self.x - parent[0] * self.facing, self.y + parent[1]];
        for (local, facing, kind) in [(sides, -1, 0), (sides, 1, 0), (up.unwrap_or(&[]), -1, 2)] {
            if local.is_empty() {
                break;
            }
            let mut hit = HeroHit::polygon(source, Class::Spell, local, at, facing);
            hit.damage = damage;
            hit.kind = kind;
            attacks.push(hit);
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
    fn quick_cast_casts_on_the_press_and_never_focuses() {
        let (mut c, k) = (Cast::new(), knight());
        c.has_fireball = true;
        let mut soul = 99;
        assert!(c.quick(P, true, &mut soul, &k, false));
        assert_eq!(c.phase, CastPhase::Antic);
        assert_eq!(soul, 99 - P.cost);
        // Held down it is one press, not a cast per tick.
        for _ in 0..ANTIC + CAST + 4 {
            c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
            assert!(!c.quick(P, true, &mut soul, &k, false));
        }
        assert_eq!(c.phase, CastPhase::Off);
        assert_eq!(soul, 99 - P.cost);
        // Let go and press again: a second cast.
        c.quick(P, false, &mut soul, &k, false);
        assert!(c.quick(P, true, &mut soul, &k, false));
        assert_eq!(soul, 99 - 2 * P.cost);
    }
    #[test]
    fn quick_cast_obeys_the_same_gates_as_a_tap() {
        let (mut c, k) = (Cast::new(), knight());
        let mut soul = 99;
        assert!(!c.quick(P, true, &mut soul, &k, false), "no spell yet");
        c.has_fireball = true;
        c.quick(P, false, &mut soul, &k, false);
        assert!(!c.quick(P, true, &mut soul, &k, true), "blocked");
        c.quick(P, false, &mut soul, &k, false);
        let mut dry = P.cost - 1;
        assert!(!c.quick(P, true, &mut dry, &k, false), "too little SOUL");
        assert_eq!(dry, P.cost - 1);
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
    // Desolate Dive and Howling Wraiths, with the cooked level-1 values.
    const Q: QuakeParams = QuakeParams {
        antic_ticks: 15,
        antic_rise: 684851,
        fall_speed: 3276800,
        land_ticks: 45,
        invuln_ticks: 24,
        fall_damage: 15,
        fall_bounds: [-32768, -100948, 32768, 41291],
        slam_damage: 20,
        slam_from: 6,
        slam_to: 12,
        slam_at: [1966, 53860],
        slam: &[
            [-188054, 26926],
            [-275, 66990],
            [-2426, -152121],
            [-525008, -152121],
            [-412564, -75520],
        ],
    };
    const S: ScreamParams = ScreamParams {
        antic_ticks: 6,
        cast_ticks: 30,
        hit_from: 3,
        hit_to: 27,
        damage: 13,
        at: [0, 169738],
        sides: &[
            [-234078, 1854],
            [-225409, 130973],
            [-105649, 188160],
            [-3528, -284975],
            [-237540, -53879],
        ],
        up: &[
            [-3581, 237423],
            [-106257, 183374],
            [1858, -227436],
            [97384, 191742],
        ],
    };
    const MOVE: crate::Params = crate::Params {
        gravity: 48 * ONE,
        fall: 20 * ONE,
        half_width: ONE / 4,
        bottom: -ONE,
        top: ONE / 4,
        ..crate::Params::ZERO
    };
    const FLOOR: [i32; 4] = [-50 * ONE, 0, 50 * ONE, 0];
    /// One frame as the guest runs it: the spell's clock, then the step.
    fn frame(k: &mut Player) -> SpellCue {
        let cue = k.spell_tick(&Q, &S);
        k.step(MOVE, 0, false, 1, |_| FLOOR);
        cue
    }
    fn standing() -> Player {
        let mut k = Player::spawn(0, ONE);
        k.step(MOVE, 0, false, 1, |_| FLOOR);
        assert!(k.grounded);
        k
    }
    fn hits(k: &Player) -> HeroAttacks {
        let mut a = HeroAttacks::new();
        k.spell_hits(&Q, &S, &mut a);
        a
    }
    #[test]
    fn spell_choice_reads_the_stick_at_the_press_and_falls_back_to_the_fireball() {
        let k = knight();
        let mut c = Cast::new();
        (c.has_fireball, c.has_quake) = (true, true);
        let mut soul = 99;
        // Down held as the button goes down, let go before the release: still Dive.
        c.aim = -1;
        c.tick(P, ANTIC, CAST, true, &mut soul, &k, false);
        c.aim = 0;
        c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        assert_eq!(c.take_chosen(), Some(Choice::Quake));
        assert_eq!(c.phase, CastPhase::Off, "the fireball stays put");
        assert_eq!(soul, 99, "Dive pays at Level Check 2, not at the press");
        // Up without Wraiths is `Has Scream?`'s CANCEL into `Has Fireball?`.
        c.aim = 1;
        c.tick(P, ANTIC, CAST, true, &mut soul, &k, false);
        c.tick(P, ANTIC, CAST, false, &mut soul, &k, false);
        assert_eq!(c.take_chosen(), None);
        assert_eq!(c.phase, CastPhase::Antic);
        assert_eq!(soul, 99 - P.cost);
    }
    #[test]
    fn dive_and_wraiths_obey_the_cast_gates() {
        let mut k = knight();
        let mut c = Cast::new();
        c.has_quake = true;
        c.aim = -1;
        let mut soul = P.cost - 1;
        assert!(!c.quick(P, true, &mut soul, &k, false), "too little SOUL");
        c.quick(P, false, &mut soul, &k, false);
        soul = 99;
        c.has_quake = false;
        assert!(
            !c.quick(P, true, &mut soul, &k, false),
            "not owned and no fireball"
        );
        c.quick(P, false, &mut soul, &k, false);
        c.has_quake = true;
        k.spell = HeroSpell::Scream(3);
        assert!(
            !c.quick(P, true, &mut soul, &k, false),
            "one spell at a time"
        );
        c.quick(P, false, &mut soul, &k, false);
        k.spell = HeroSpell::Off;
        assert!(c.quick(P, true, &mut soul, &k, false));
        assert_eq!(c.take_chosen(), Some(Choice::Quake));
        assert_eq!(c.take_chosen(), None, "taken once");
    }
    #[test]
    fn dive_rises_falls_lands_and_hands_back_control() {
        let mut k = standing();
        let y0 = k.y;
        k.start_quake(&Q);
        assert!(k.casting() && !k.ignores_contact(), "the antic takes hits");
        let mut fall_at = None;
        for t in 1..=Q.antic_ticks {
            if frame(&mut k) == SpellCue::Fall {
                fall_at = Some(t);
            }
        }
        assert_eq!(
            fall_at,
            Some(Q.antic_ticks),
            "TakeMP after the antic's clip"
        );
        assert!(k.y > y0, "Q On Ground rises");
        assert_eq!(k.spell, HeroSpell::QuakeDown);
        assert!(k.ignores_contact(), "HAZARD_ONLY from Quake1 Down");
        // The first falling step runs at the fall speed, not gravity's.
        let y = k.y;
        frame(&mut k);
        assert_eq!(y - k.y, Q.fall_speed / 60);
        let mut n = 0;
        while k.spell == HeroSpell::QuakeDown {
            frame(&mut k);
            n += 1;
            assert!(n < 60);
        }
        assert_eq!(k.spell, HeroSpell::QuakeLand(0), "HERO LANDED on the floor");
        assert!(k.grounded && k.y == y0);
        for _ in 1..Q.land_ticks {
            frame(&mut k);
            assert!(k.grounded && k.ignores_contact());
        }
        frame(&mut k);
        assert_eq!(k.spell, HeroSpell::Off);
        assert_eq!(k.quake_invuln, Q.invuln_ticks, "QuakeInvuln");
        for _ in 0..Q.invuln_ticks {
            assert!(k.ignores_contact());
            frame(&mut k);
        }
        assert!(!k.ignores_contact());
    }
    #[test]
    fn a_dive_from_the_air_hovers_through_its_antic() {
        let mut k = Player::spawn(0, 8 * ONE);
        k.start_quake(&Q);
        let y = k.y;
        for _ in 0..Q.antic_ticks - 1 {
            frame(&mut k);
        }
        assert_eq!(k.y, y, "Q Off Ground: no rise, no gravity");
    }
    #[test]
    fn dive_hits_are_the_fall_box_then_the_slam_window() {
        let mut k = standing();
        k.facing = 1;
        k.spell = HeroSpell::QuakeDown;
        let a = hits(&k);
        let fall = a.iter().next().unwrap();
        assert_eq!(a.iter().count(), 1);
        assert_eq!(
            (fall.class, fall.source, fall.damage),
            (Class::Spell, Source::Quake, 15)
        );
        assert_eq!(
            fall.bounds,
            [k.x - ONE / 2, k.y - 100948, k.x + ONE / 2, k.y + 41291]
        );
        for t in 0..Q.land_ticks {
            k.spell = HeroSpell::QuakeLand(t);
            let a = hits(&k);
            let live = (Q.slam_from..Q.slam_to).contains(&t);
            assert_eq!(a.iter().count(), if live { 2 } else { 0 }, "tick {t}");
        }
        k.spell = HeroSpell::QuakeLand(Q.slam_from);
        let a = hits(&k);
        let [l, r] = [a.iter().next().unwrap(), a.iter().nth(1).unwrap()];
        assert_eq!((l.damage, l.facing, r.facing), (Q.slam_damage, -1, 1));
        // Hit L and Hit R mirror each other about Q Slam, eight units each way.
        let centre = k.x - Q.slam_at[0];
        assert_eq!(centre - l.bounds[0], r.bounds[2] - centre);
        assert!(l.bounds[0] < k.x - 7 * ONE && r.bounds[2] > k.x + 7 * ONE);
        assert!(l.hits([k.x - 4 * ONE, k.y - ONE, k.x - 4 * ONE + 1, k.y]));
        assert!(!l.hits([k.x + 4 * ONE, k.y - ONE, k.x + 4 * ONE + 1, k.y]));
        // The world-fixed slam: turning round moves only the small parent offset.
        k.facing = -1;
        let a = hits(&k);
        assert_eq!(a.iter().next().unwrap().facing, -1);
        assert_eq!(
            a.iter().next().unwrap().bounds[0],
            l.bounds[0] + 2 * Q.slam_at[0]
        );
    }
    #[test]
    fn wraiths_hover_burst_and_hit_three_ways_in_their_window() {
        let mut k = Player::spawn(0, 8 * ONE);
        k.start_scream();
        let y = k.y;
        let mut burst = None;
        let mut live = 0;
        let mut t = 0;
        while k.casting() {
            let a = hits(&k);
            if a.iter().count() != 0 {
                live += 1;
                assert_eq!(a.iter().count(), 3);
                let up = a.iter().nth(2).unwrap();
                assert_eq!((up.cardinal(), up.damage), (1, S.damage));
                assert_eq!(a.iter().next().unwrap().cardinal(), 2);
                assert_eq!(a.iter().nth(1).unwrap().cardinal(), 0);
            }
            t += 1;
            if frame(&mut k) == SpellCue::Burst {
                burst = Some(t);
            }
            if k.casting() {
                assert_eq!(k.y, y, "Wraiths hang in the air");
            }
        }
        assert_eq!(burst, Some(S.antic_ticks));
        assert_eq!(t, S.antic_ticks + S.cast_ticks);
        assert_eq!(live, S.hit_to - S.hit_from);
        assert!(!k.ignores_contact(), "Wraiths take hits");
    }
    #[test]
    fn a_hit_cancels_the_spell_and_its_hits() {
        let mut k = Player::spawn(0, 8 * ONE);
        k.start_scream();
        for _ in 0..S.antic_ticks + S.hit_from {
            frame(&mut k);
        }
        assert_eq!(hits(&k).iter().count(), 3);
        k.take_hit();
        assert!(!k.casting());
        assert_eq!(hits(&k).iter().count(), 0);
    }
    #[test]
    fn no_dash_or_crystal_heart_while_a_spell_runs() {
        let mut k = standing();
        (k.has_dash, k.has_super_dash) = (true, true);
        k.start_scream();
        let p = crate::Params {
            dash_ticks: 10,
            ..MOVE
        };
        k.dash_input(p, true);
        k.super_dash_input(p, true);
        assert_eq!((k.dash_left, k.super_dash), (0, crate::SuperDash::Off));
    }
}
