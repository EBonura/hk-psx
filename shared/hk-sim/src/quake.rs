//! Desolate Dive and Descending Dark, from the Hero's `Spell Control` FSM (`Quake Antic`, `Quake1 Down`,
//! `Quake1 Land`, `Quake Finish` and their level 2 twins) and the `Q *` children of the Knight prefab.
//!
//! Cast on a tap of the spell button with Down held at the press. The Knight hops (11 units a second
//! from the ground, none from the air, slowing by `DecelerateV2` 0.95) while the cast clip plays, with
//! gravity and control off; the soul is paid; he falls at a constant speed, hurting what he falls
//! through (`Q Fall Damage`) and refusing enemy damage (`damageMode` 1); on landing the slam's
//! polygons hurt for a few frames (`Q Slam`, and for Descending Dark `Q Slam 2` then the pillar
//! `Q Mega`), he is held for the landing clip, and `QuakeInvuln` gives him 0.4 s more of refusing
//! enemy damage.
use crate::strike::{HitSpec, Strike};
use crate::Player;

/// Values the cook reads from the install (host/hk-cook ability_params.rs).
#[derive(Clone, Copy)]
pub struct QuakeParams {
    /// `Quake Antic`'s clip, in ticks.
    pub antic_ticks: u8,
    /// `Quake Antic Speed` from the ground, Q16 units a second upward.
    pub hop_speed: i32,
    /// What `DecelerateV2(0.95)` leaves of the hop's speed each tick (Q16): it slows by 0.95 every
    /// 50 Hz physics step.
    pub hop_keep: i32,
    /// The fall's constant speed, Q16 units a second downward.
    pub fall_speed: i32,
    /// Ticks the landing holds him for, by level: `Quake1 Land`'s wait and `Quake Land 2`'s clip.
    pub land_ticks: [u8; 2],
    /// `QuakeInvuln`: ticks of refusing enemy damage after the landing.
    pub invuln_ticks: u8,
    /// `Q Fall Damage`: a box down the Knight's body while he falls.
    pub fall: HitSpec,
    /// `Q Slam` (level 1) or `Q Slam 2` (level 2), left and right of him, from the landing.
    pub slam: [[HitSpec; 2]; 2],
    /// `Q Mega`, the pillar of Descending Dark, left and right, from its own start (`pillar_at`).
    pub pillar: [HitSpec; 2],
    /// Ticks into the landing that the pillar starts (`Quake Land 2`'s trigger frame).
    pub pillar_at: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuakePhase {
    Off,
    /// `Quake Antic`: the hop, no gravity, no control.
    Antic,
    /// `Quake1 Down` / `Quake2 Down`: falling.
    Fall,
    /// `Quake1 Land` ... `Quake Finish`: the slam and the hold.
    Land,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quake {
    pub phase: QuakePhase,
    /// Ticks into the phase.
    pub tick: u16,
    /// 1 for Desolate Dive, 2 for Descending Dark: the spell's level when it was cast.
    pub level: u8,
    /// Ticks of `QuakeInvuln` left once the landing has let go of him.
    pub invuln: u16,
    hop: i32,
}
impl Default for Quake {
    fn default() -> Self {
        Self::new()
    }
}
impl Quake {
    pub const fn new() -> Self {
        Self { phase: QuakePhase::Off, tick: 0, level: 0, invuln: 0, hop: 0 }
    }
    /// The Knight is pinned from the cast to the end of the landing.
    pub fn locks_control(&self) -> bool {
        self.phase != QuakePhase::Off
    }
    /// `damageMode` 1 and `parryInvulnTimer`: `TakeDamage` refuses an enemy's hit (a hazard still hurts).
    pub fn refuses_enemies(&self) -> bool {
        matches!(self.phase, QuakePhase::Fall | QuakePhase::Land) || self.invuln > 0
    }
    /// Start the spell at `level` from the ground (`grounded`) or the air.
    pub fn begin(&mut self, p: &QuakeParams, level: u8, grounded: bool) {
        *self = Self {
            phase: QuakePhase::Antic,
            tick: 0,
            level,
            invuln: self.invuln,
            hop: if grounded { p.hop_speed } else { 0 },
        };
    }
    /// Abandon the spell (the Knight was hit out of it, a scene ended).
    pub fn cancel(&mut self) {
        *self = Self::new();
    }
    /// The vertical velocity the Knight is driven with this tick (Q16 units a second), `None` when the
    /// spell does not own it. Gravity is off for the whole spell.
    pub fn velocity(&mut self, p: &QuakeParams) -> Option<i32> {
        match self.phase {
            QuakePhase::Off => None,
            QuakePhase::Antic => {
                let v = self.hop;
                self.hop = psx_math::int32::mul_div_i32(self.hop, p.hop_keep, 65536);
                Some(v)
            }
            QuakePhase::Fall => Some(-p.fall_speed),
            QuakePhase::Land => Some(0),
        }
    }
    /// One tick, after the Knight has moved. Returns true on the tick the landing starts (the slam,
    /// the shake and the sound).
    pub fn tick(&mut self, p: &QuakeParams, player: &Player) -> bool {
        self.invuln = self.invuln.saturating_sub(1);
        match self.phase {
            QuakePhase::Off => false,
            QuakePhase::Antic => {
                self.tick += 1;
                if self.tick >= u16::from(p.antic_ticks) {
                    self.phase = QuakePhase::Fall;
                    self.tick = 0;
                }
                false
            }
            QuakePhase::Fall => {
                self.tick += 1;
                // `HERO LANDED`: the floor under him (the cast from a floor's edge lands at once).
                if player.grounded {
                    self.phase = QuakePhase::Land;
                    self.tick = 0;
                    return true;
                }
                false
            }
            QuakePhase::Land => {
                self.tick += 1;
                if self.tick >= u16::from(p.land_ticks[usize::from(self.level.clamp(1, 2)) - 1]) {
                    self.phase = QuakePhase::Off;
                    self.tick = 0;
                    self.invuln = u16::from(p.invuln_ticks);
                }
                false
            }
        }
    }
    /// The shapes that hurt this tick, written to `out`: how many.
    pub fn strikes(&self, p: &QuakeParams, player: &Player, out: &mut [Strike]) -> usize {
        let mut n = 0;
        let mut push = |s: Strike| {
            if n < out.len() {
                out[n] = s;
                n += 1;
            }
        };
        match self.phase {
            QuakePhase::Fall => push(p.fall.place(player.x, player.y, player.facing)),
            QuakePhase::Land => {
                let level = usize::from(self.level.clamp(1, 2)) - 1;
                for h in &p.slam[level] {
                    if h.live(self.tick) {
                        push(h.place(player.x, player.y, player.facing));
                    }
                }
                if level == 1 && self.tick >= u16::from(p.pillar_at) {
                    for h in &p.pillar {
                        if h.live(self.tick - u16::from(p.pillar_at)) {
                            push(h.place(player.x, player.y, player.facing));
                        }
                    }
                }
            }
            _ => {}
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;
    const SQUARE: HitSpec = HitSpec {
        points: [[-256, 0], [256, 0], [256, 256], [-256, 256], [0; 2]],
        len: 4,
        damage: 20,
        start: 6,
        end: 12,
    };
    const P: QuakeParams = QuakeParams {
        antic_ticks: 15,
        hop_speed: 11 * ONE,
        hop_keep: 62_800,
        fall_speed: 50 * ONE,
        land_ticks: [45, 42],
        invuln_ticks: 24,
        fall: HitSpec { damage: 15, start: 0, end: 255, ..SQUARE },
        slam: [[SQUARE; 2], [HitSpec { damage: 35, ..SQUARE }; 2]],
        pillar: [HitSpec { damage: 15, start: 6, end: 18, ..SQUARE }; 2],
        pillar_at: 21,
    };
    fn knight() -> Player {
        Player::spawn(0, 0)
    }
    #[test]
    fn the_hop_slows_and_the_fall_is_constant() {
        let (mut q, k) = (Quake::new(), knight());
        q.begin(&P, 1, true);
        assert!(q.locks_control());
        let first = q.velocity(&P).unwrap();
        assert_eq!(first, 11 * ONE, "from the ground he rises at 11 units a second");
        q.tick(&P, &k);
        let second = q.velocity(&P).unwrap();
        assert!(second < first && second > 0, "slowing by DecelerateV2");
        for _ in 0..P.antic_ticks - 1 {
            q.tick(&P, &k);
        }
        assert_eq!(q.phase, QuakePhase::Fall, "the antic clip's end starts the fall");
        assert_eq!(q.velocity(&P), Some(-50 * ONE));
    }
    #[test]
    fn from_the_air_there_is_no_hop() {
        let mut q = Quake::new();
        q.begin(&P, 1, false);
        assert_eq!(q.velocity(&P), Some(0));
    }
    #[test]
    fn a_landing_slams_holds_and_leaves_invulnerability() {
        let (mut q, mut k) = (Quake::new(), knight());
        q.begin(&P, 1, false);
        for _ in 0..P.antic_ticks {
            q.tick(&P, &k);
        }
        assert!(q.refuses_enemies(), "damageMode 1 for the whole fall");
        let mut out = [Strike::NONE; 4];
        assert_eq!(q.strikes(&P, &k, &mut out), 1, "the fall's box");
        assert_eq!(out[0].damage, 15);
        k.grounded = true;
        assert!(q.tick(&P, &k), "the landing");
        assert_eq!(q.phase, QuakePhase::Land);
        let mut live = 0;
        for _ in 0..P.land_ticks[0] {
            assert!(q.locks_control());
            live += usize::from(q.strikes(&P, &k, &mut out) > 0);
            q.tick(&P, &k);
        }
        assert_eq!(live, 6, "the slam hurts from tick 6 up to 12");
        assert_eq!(q.phase, QuakePhase::Off);
        assert!(q.refuses_enemies(), "QuakeInvuln");
        for _ in 0..P.invuln_ticks {
            q.tick(&P, &k);
        }
        assert!(!q.refuses_enemies());
    }
    #[test]
    fn descending_dark_adds_its_pillar_after_the_slam() {
        let (mut q, mut k) = (Quake::new(), knight());
        q.begin(&P, 2, false);
        for _ in 0..P.antic_ticks {
            q.tick(&P, &k);
        }
        k.grounded = true;
        q.tick(&P, &k);
        let mut out = [Strike::NONE; 8];
        let (mut slam, mut pillar) = (0, 0);
        for age in 0..u16::from(P.land_ticks[1]) {
            let n = q.strikes(&P, &k, &mut out);
            let damage: u16 = out[..n].iter().map(|s| s.damage).sum();
            if (6..12).contains(&age) && damage == 70 {
                slam += 1;
            }
            if age >= u16::from(P.pillar_at) + 6 && damage == 30 {
                pillar += 1;
            }
            q.tick(&P, &k);
        }
        assert_eq!(slam, 6, "both slam halves at 35");
        assert!(pillar > 0, "then the pillar's two halves at 15");
    }
}
