//! Optional session cheats. Disabled settings delegate to normal gameplay.
//! Toggles survive death/Select reset; only Reset All turns them off. Disabling
//! max masks clamps excess red health; earned blue masks remain independent.
use hk_sim::{Hurt, VitalParams, Vitals};
// Windows PlayerData.AddGGPlayerDataOverrides sets nailDamage=21 and upgrades=4.
pub const PURE_NAIL_DAMAGE: u16 = 21;
pub const MAX_MASKS: u16 = 9;
pub const BLUE_CHEAT_CAP: u16 = 20;
/// Toggle rows on the Cheats page, ahead of Restore, Add Lifeblood, Reset and Back.
pub const CHEAT_ROWS: usize = 12;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub invincible: bool,
    pub max_nail: bool,
    pub infinite_soul: bool,
    pub max_masks: bool,
    /// No admitted scene grants the Mothwing Cloak, Mantis Claw or Monarch
    /// Wings, so these are the only way to reach them until the ability
    /// pickups exist.
    pub dash: bool,
    pub claw: bool,
    pub wings: bool,
    pub heart: bool,
    pub cloak: bool,
    pub dream: bool,
    pub spell: bool,
    /// Marks every charm found. Only Grubsong's Shiny is in the admitted
    /// scenes and it sits inactive, so this is the only route to the board.
    /// It grants nothing else: the notch budget and every refusal still apply.
    pub charms: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Restore,
    AddBlue,
    Reset,
}
impl Settings {
    pub const fn new() -> Self {
        Self {
            invincible: false,
            max_nail: false,
            infinite_soul: false,
            max_masks: false,
            dash: false,
            claw: false,
            wings: false,
            heart: false,
            cloak: false,
            dream: false,
            spell: false,
            charms: false,
        }
    }
    pub fn enabled(&self, row: usize) -> bool {
        match row {
            0 => self.invincible,
            1 => self.max_nail,
            2 => self.infinite_soul,
            3 => self.max_masks,
            4 => self.dash,
            5 => self.claw,
            6 => self.wings,
            7 => self.heart,
            8 => self.cloak,
            9 => self.dream,
            10 => self.spell,
            11 => self.charms,
            _ => false,
        }
    }
    pub fn adjust(&mut self, row: usize, delta: i8) {
        if delta == 0 {
            return;
        }
        let value = match row {
            0 => &mut self.invincible,
            1 => &mut self.max_nail,
            2 => &mut self.infinite_soul,
            3 => &mut self.max_masks,
            4 => &mut self.dash,
            5 => &mut self.claw,
            6 => &mut self.wings,
            7 => &mut self.heart,
            8 => &mut self.cloak,
            9 => &mut self.dream,
            10 => &mut self.spell,
            11 => &mut self.charms,
            _ => return,
        };
        *value = delta > 0;
    }
    pub fn bits(self) -> u32 {
        u32::from(self.invincible)
            | (u32::from(self.max_nail) << 1)
            | (u32::from(self.infinite_soul) << 2)
            | (u32::from(self.max_masks) << 3)
            | (u32::from(self.dash) << 4)
            | (u32::from(self.claw) << 5)
            | (u32::from(self.wings) << 6)
            | (u32::from(self.heart) << 7)
            | (u32::from(self.cloak) << 8)
            | (u32::from(self.dream) << 9)
            | (u32::from(self.spell) << 10)
            | (u32::from(self.charms) << 11)
    }
    /// The live parameter stack: the cooked base, then the PlayerData Sly's
    /// counter moves (`maxHealthBase` and the SOUL reserve), then the equipped
    /// charms, then the cheats. Composing here rather than at each consumer is
    /// what keeps a bonus from being applied twice, and the shop comes before
    /// the charms because the source recomputes `maxHealth` as `maxHealthBase`
    /// plus the charm bonuses rather than the other way round.
    pub fn params(self, base: VitalParams) -> VitalParams {
        let mut base = crate::charms::vitals(crate::shop::vitals(base));
        if self.max_nail {
            base.nail_damage = PURE_NAIL_DAMAGE;
        }
        if self.max_masks {
            base.max_health = MAX_MASKS;
        }
        base
    }
    pub fn new_vitals(self, base: VitalParams) -> Vitals {
        let mut v = Vitals::new(self.params(base));
        self.maintain(&mut v, base);
        v
    }
    /// Called before/after consumption so Focus retains its ordinary timing
    /// and healing rules, while the displayed resource stays full.
    pub fn maintain(self, v: &mut Vitals, base: VitalParams) {
        if self.infinite_soul && !v.dead {
            v.soul = base.max_soul;
        }
    }
    pub fn apply(self, previous: Self, v: &mut Vitals, base: VitalParams, action: Action) {
        if self.charms != previous.charms {
            crate::charms::grant_all(self.charms);
        }
        let p = self.params(base);
        if self.max_masks != previous.max_masks && !v.dead {
            if self.max_masks {
                v.health = p.max_health;
            } else {
                v.health = v.health.min(p.max_health);
            }
        }
        if !v.dead {
            match action {
                Action::Restore => {
                    v.health = p.max_health;
                    v.soul = p.max_soul;
                }
                // Do not reduce blue health already obtained from normal gameplay.
                Action::AddBlue => {
                    if v.blue_health < BLUE_CHEAT_CAP {
                        v.blue_health = v.blue_health.saturating_add(5).min(BLUE_CHEAT_CAP);
                    }
                }
                _ => {}
            }
        }
        self.maintain(v, base);
    }
    pub fn hurt(
        self,
        v: &mut Vitals,
        base: VitalParams,
        damage: u16,
        direction: i32,
        hazard: bool,
        shadow: bool,
    ) -> Hurt {
        // Source TakeDamage returns immediately while cState.shadowDashing, so
        // a Shade Cloak dash passes through a hazard rather than respawning at
        // it. That is deliberately unlike the Invincibility cheat below.
        if shadow {
            return Hurt::Ignored;
        }
        if !self.invincible {
            let p = self.params(base);
            let hurt = v.hurt(p, damage, direction, hazard);
            // HeroController::TakeDamage runs TakeDamageCharmEffects only for a
            // hit that actually landed, so an ignored hit charges no SOUL.
            if hurt != Hurt::Ignored {
                crate::charms::on_damage(v, p);
            }
            return hurt;
        }
        if !hazard || damage == 0 || v.dead || v.hazard_pending {
            return Hurt::Ignored;
        }
        // Invulnerability must not strand the Knight below the room or in
        // spikes: keep the existing safe-position recovery, without damage.
        v.recoil_direction = direction.clamp(-1, 1);
        v.hazard_pending = true;
        v.freeze_ticks = 0;
        v.recoil_ticks = 0;
        Hurt::Hazard
    }
}
#[no_mangle]
pub static mut HK_CHEATS: u32 = 0;
#[no_mangle]
pub static mut HK_CHEAT_NAIL_DAMAGE: u32 = 0;
#[no_mangle]
pub static mut HK_CHEAT_MASK_CAP: u32 = 0;
pub fn publish(settings: Settings, base: VitalParams) {
    let p = settings.params(base);
    unsafe {
        HK_CHEATS = settings.bits();
        HK_CHEAT_NAIL_DAMAGE = p.nail_damage as u32;
        HK_CHEAT_MASK_CAP = p.max_health as u32;
    }
}
