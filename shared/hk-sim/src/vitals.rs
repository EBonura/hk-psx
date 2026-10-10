//! No-charm health and damage lifecycle using cooked Windows source values.
//! FreezeMoment's time-scale ramps are represented by a bounded fixed-step hold;
//! presentation and exact original wall-clock timing are not reproduced here.

#[derive(Clone, Copy, Debug)]
pub struct VitalParams {
    pub max_health: u16,
    pub max_soul: u16,
    pub nail_damage: u16,
    pub soul_per_hit: u16,
    pub invulnerable_ticks: u16,
    pub hazard_invulnerable_ticks: u16,
    pub recoil_ticks: u16,
    pub freeze_ticks: u16,
    pub death_ticks: u16,
    pub recoil_speed: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hurt {
    Ignored,
    Recoiling,
    Hazard,
    Died,
}
/// `InvulnerablePulse.pulseDuration` on the Knight (0.1 s) in 60 Hz ticks.
pub const PULSE_TICKS: u16 = 6;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vitals {
    pub health: u16,
    /// Temporary Lifeblood; consumed before normal masks, never Focus-healed.
    pub blue_health: u16,
    pub soul: u16,
    pub invulnerable_ticks: u16,
    /// What `invulnerable_ticks` was set to, so `invulnerable_pulse` knows how
    /// long the current invulnerability has run.
    pub invulnerable_from: u16,
    pub recoil_ticks: u16,
    pub freeze_ticks: u16,
    pub death_ticks: u16,
    pub dead: bool,
    pub hazard_pending: bool,
    pub recoil_direction: i32,
}
impl Vitals {
    pub const fn new(p: VitalParams) -> Self {
        Self {
            health: p.max_health,
            blue_health: 0,
            soul: 0,
            invulnerable_ticks: 0,
            invulnerable_from: 0,
            recoil_ticks: 0,
            freeze_ticks: 0,
            death_ticks: 0,
            dead: false,
            hazard_pending: false,
            recoil_direction: 0,
        }
    }
    /// Call once each VBlank before damage checks. A freeze tick suppresses
    /// world simulation; rendering/input may continue independently.
    pub fn tick(&mut self) -> bool {
        if self.freeze_ticks != 0 {
            self.freeze_ticks -= 1;
            return true;
        }
        self.invulnerable_ticks = self.invulnerable_ticks.saturating_sub(1);
        self.recoil_ticks = self.recoil_ticks.saturating_sub(1);
        self.death_ticks = self.death_ticks.saturating_sub(1);
        false
    }
    /// How dark the Knight is drawn while invulnerable, 0 (normal) to
    /// `PULSE_TICKS` (black): the original's `InvulnerablePulse`. Its
    /// `Invulnerable` coroutine waits out `invulnerableFreezeDuration`
    /// (DAMAGE_FREEZE_DOWN, the one tick the cook adds to INVUL_TIME) and then
    /// pulses until the invulnerability ends; each `Update` moves the timer
    /// one frame toward `pulseDuration` (0.1 s), clamps there and turns back,
    /// clamps at 0 and turns again, and the sprite colour is
    /// `Lerp(normal, invulColor = black, timer / pulseDuration)`. The timer
    /// runs on scaled time, so it stops with the damage freeze, as
    /// `invulnerable_ticks` does. One step per simulation tick.
    pub fn invulnerable_pulse(&self) -> u16 {
        if self.invulnerable_ticks == 0 || self.dead {
            return 0;
        }
        let elapsed = self
            .invulnerable_from
            .saturating_sub(self.invulnerable_ticks);
        // Updates since startInvulnerablePulse: the first tick is the freeze-down.
        let Some(step) = elapsed.checked_sub(1) else {
            return 0;
        };
        // Timer after `step` Updates: 1..=P up, P again (clamped), P-1..=0
        // down, 0 again (clamped); period 2P+2.
        let p = PULSE_TICKS;
        match step % (2 * p + 2) {
            r if r <= p => r,
            r if r == p + 1 => p,
            r => 2 * p + 1 - r,
        }
    }
    pub fn can_control(&self) -> bool {
        !self.dead && !self.hazard_pending && self.recoil_ticks == 0 && self.freeze_ticks == 0
    }
    /// `direction` is the knockback direction away from the impact (±1).
    /// Source hazards can bypass normal contact invulnerability, but never
    /// trigger repeatedly during an already-running hazard respawn/death.
    pub fn hurt(&mut self, p: VitalParams, damage: u16, direction: i32, hazard: bool) -> Hurt {
        if damage == 0
            || self.dead
            || self.hazard_pending
            || (!hazard
                && (self.invulnerable_ticks != 0
                    || self.recoil_ticks != 0
                    || self.freeze_ticks != 0))
        {
            return Hurt::Ignored;
        }
        // PlayerData.TakeHealth consumes healthBlue then recursively applies
        // only positive overflow to ordinary health. Keep this AFTER immunity.
        let normal_damage = damage.saturating_sub(self.blue_health);
        self.blue_health = self.blue_health.saturating_sub(damage);
        self.health = self.health.saturating_sub(normal_damage);
        self.recoil_direction = direction.clamp(-1, 1);
        if self.health == 0 {
            self.dead = true;
            self.death_ticks = p.death_ticks;
            self.freeze_ticks = 0;
            self.recoil_ticks = 0;
            self.invulnerable_ticks = 0;
            return Hurt::Died;
        }
        if hazard {
            self.hazard_pending = true;
            self.freeze_ticks = 0;
            self.recoil_ticks = 0;
            return Hurt::Hazard;
        }
        self.freeze_ticks = p.freeze_ticks;
        self.recoil_ticks = p.recoil_ticks;
        self.invulnerable_ticks = p.invulnerable_ticks;
        self.invulnerable_from = p.invulnerable_ticks;
        Hurt::Recoiling
    }
    /// Original StartRecoil sets (±speed, speed/2) with gravity disabled.
    /// The caller uses its normal swept collision solver and faces the impact.
    pub fn recoil_velocity(&self, p: VitalParams) -> Option<(i32, i32)> {
        if self.dead || self.hazard_pending || self.freeze_ticks != 0 || self.recoil_ticks == 0 {
            None
        } else {
            Some((self.recoil_direction * p.recoil_speed, p.recoil_speed / 2))
        }
    }
    pub fn needs_death_respawn(&self) -> bool {
        self.dead && self.death_ticks == 0
    }
    /// Position/marker/fade handling belongs to the room runtime. This preserves
    /// the health loss and SOUL rather than treating a spike as a fresh new game.
    pub fn finish_hazard_respawn(&mut self, p: VitalParams) {
        if self.hazard_pending && !self.dead {
            self.hazard_pending = false;
            self.invulnerable_ticks = p.hazard_invulnerable_ticks;
            self.invulnerable_from = p.hazard_invulnerable_ticks;
        }
    }
    /// Original ADD BLUE HEALTH adds one, independent of max normal health.
    pub fn add_blue_health(&mut self, amount: u16) {
        if !self.dead {
            self.blue_health = self.blue_health.saturating_add(amount);
        }
    }
    /// No-charm PlayerData.UpdateBlueHealth, called at bench/full reset.
    pub fn reset_blue_health(&mut self) {
        self.blue_health = 0;
    }
    pub fn gain_soul_on_nail_hit(&mut self, p: VitalParams) {
        if !self.dead {
            self.soul = self.soul.saturating_add(p.soul_per_hit).min(p.max_soul);
        }
    }
    /// `AddMPCharge`, as the Dream Nail's reward uses it.
    pub fn add_soul(&mut self, p: VitalParams, amount: u16) {
        self.soul = self.soul.saturating_add(amount).min(p.max_soul);
    }
    pub fn take_soul(&mut self, amount: u16) {
        self.soul = self.soul.saturating_sub(amount);
    }
    /// AddHealth event only. The source Focus FSM's drain/cancel timing must be
    /// implemented before this is wired to a held input button.
    pub fn heal(&mut self, p: VitalParams, amount: u16) {
        if !self.dead && !self.hazard_pending {
            self.health = self.health.saturating_add(amount).min(p.max_health);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: VitalParams = VitalParams {
        max_health: 5,
        max_soul: 99,
        nail_damage: 5,
        soul_per_hit: 11,
        invulnerable_ticks: 79,
        hazard_invulnerable_ticks: 40,
        recoil_ticks: 12,
        freeze_ticks: 19,
        death_ticks: 171,
        recoil_speed: 15 * 65536,
    };
    #[test]
    fn sustained_contact_cannot_repeatedly_drain_health() {
        let mut v = Vitals::new(P);
        assert_eq!(v.hurt(P, 1, -1, false), Hurt::Recoiling);
        for _ in 0..P.freeze_ticks + P.invulnerable_ticks - 1 {
            v.tick();
            assert_eq!(v.hurt(P, 1, 1, false), Hurt::Ignored);
        }
        assert_eq!(v.health, 4);
        v.tick();
        assert_eq!(v.hurt(P, 1, 1, false), Hurt::Recoiling);
        assert_eq!(v.health, 3);
    }
    #[test]
    fn recoil_uses_source_vector_after_freeze() {
        let mut v = Vitals::new(P);
        v.hurt(P, 1, -1, false);
        assert_eq!(v.recoil_velocity(P), None);
        for _ in 0..P.freeze_ticks {
            v.tick();
        }
        assert_eq!(v.recoil_velocity(P), Some((-15 * 65536, 15 * 32768)));
        for _ in 0..P.recoil_ticks {
            v.tick();
        }
        assert!(v.can_control());
        assert_eq!(v.recoil_velocity(P), None);
    }
    #[test]
    fn lethal_damage_is_saturating_and_death_is_one_shot() {
        let mut v = Vitals::new(P);
        assert_eq!(v.hurt(P, u16::MAX, 0, false), Hurt::Died);
        assert_eq!(v.health, 0);
        assert_eq!(v.hurt(P, 1, 0, true), Hurt::Ignored);
        v.heal(P, 5);
        assert_eq!(v.health, 0);
        for _ in 0..P.death_ticks {
            v.tick();
        }
        assert!(v.needs_death_respawn());
    }
    #[test]
    fn hazard_bypasses_contact_invulnerability_but_preserves_health_loss() {
        let mut v = Vitals::new(P);
        v.gain_soul_on_nail_hit(P);
        v.hurt(P, 1, 1, false);
        assert_eq!(v.hurt(P, 1, 1, true), Hurt::Hazard);
        assert_eq!(v.hurt(P, 1, 1, true), Hurt::Ignored);
        v.finish_hazard_respawn(P);
        assert_eq!((v.health, v.soul), (3, 11));
        assert_eq!(v.invulnerable_ticks, P.hazard_invulnerable_ticks);
    }
    #[test]
    fn soul_and_healing_saturate_without_unlocking_reserve() {
        let mut v = Vitals::new(P);
        for _ in 0..20 {
            v.gain_soul_on_nail_hit(P);
        }
        assert_eq!(v.soul, 99);
        v.take_soul(u16::MAX);
        assert_eq!(v.soul, 0);
        v.hurt(P, 2, 1, false);
        v.heal(P, u16::MAX);
        assert_eq!(v.health, 5);
    }
    #[test]
    fn lifeblood_absorbs_before_normal_health_and_preserves_recoil() {
        let mut v = Vitals::new(P);
        v.add_blue_health(2);
        assert_eq!(v.hurt(P, 1, 1, false), Hurt::Recoiling);
        assert_eq!((v.health, v.blue_health), (5, 1));
        assert_eq!(v.hurt(P, 1, 1, false), Hurt::Ignored);
        assert_eq!((v.health, v.blue_health), (5, 1));
        assert_eq!(v.hurt(P, 2, 1, true), Hurt::Hazard);
        assert_eq!((v.health, v.blue_health), (4, 0));
        v.finish_hazard_respawn(P);
        assert_eq!(v.blue_health, 0);
    }
    #[test]
    fn lifeblood_cannot_be_focus_healed_and_resets_independently() {
        let mut v = Vitals::new(P);
        v.health = 3;
        v.add_blue_health(2);
        v.heal(P, 1);
        assert_eq!((v.health, v.blue_health), (4, 2));
        v.reset_blue_health();
        assert_eq!((v.health, v.blue_health), (4, 0));
        v.add_blue_health(2);
        assert_eq!(v.hurt(P, 6, 0, true), Hurt::Died);
        assert_eq!((v.health, v.blue_health), (0, 0));
        v.add_blue_health(1);
        assert_eq!(v.blue_health, 0);
    }
    #[test]
    fn every_small_damage_matches_source_two_pool_subtraction() {
        for health in 1u16..=5 {
            for blue in 0u16..=5 {
                for damage in 0u16..=12 {
                    let mut v = Vitals::new(P);
                    v.health = health;
                    v.blue_health = blue;
                    v.hurt(P, damage, 1, false);
                    assert_eq!(v.blue_health, blue.saturating_sub(damage));
                    assert_eq!(v.health, health.saturating_sub(damage.saturating_sub(blue)));
                }
            }
        }
    }
}
