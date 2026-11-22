//! The Knight's live parameters: everything PlayerData, the equipped charms
//! and the cheats can move, in one record the game composes once per change.
//!
//! The source keeps these as HeroController constants (`RUN_SPEED_CH`,
//! `DASH_COOLDOWN_CH`, `ATTACK_COOLDOWN_TIME_CH`, ...) and FSM variables
//! (`Spell Control`'s `Time Per MP Drain` and `MP Cost`, `Fireball Control`'s
//! damage and scale) that each reader picks between by testing
//! `equippedCharm_N` at the moment it runs. Picking at every read is what the
//! port cannot afford, so the game folds the same tests into this record when
//! the board, the shop or a cheat changes, and every reader takes its value
//! from here instead of the cooked no-charm constant.
use crate::{AttackParams, FireballParams, FocusParams, NailResponseParams, VitalParams};

#[derive(Clone, Copy)]
pub struct HeroParams {
    pub vitals: VitalParams,
    pub attack: AttackParams,
    pub fireball: FireballParams,
    pub focus: FocusParams,
    pub nail_response: NailResponseParams,
    /// `HeroController::Move` on the ground: `RUN_SPEED`, or Sprintmaster's
    /// `RUN_SPEED_CH`/`RUN_SPEED_CH_COMBO`. In the air the source always runs at
    /// `RUN_SPEED`, which stays `Params::speed`.
    pub ground_speed: i32,
    /// `HeroController::HeroDash`: `DASH_COOLDOWN`, or Dashmaster's `DASH_COOLDOWN_CH`.
    pub dash_cooldown_ticks: u16,
}
impl HeroParams {
    /// The movement record a grounded or airborne step runs with.
    pub fn movement(&self, base: crate::Params, grounded: bool) -> crate::Params {
        crate::Params {
            speed: if grounded { self.ground_speed } else { base.speed },
            dash_cooldown_ticks: self.dash_cooldown_ticks,
            ..base
        }
    }
}
