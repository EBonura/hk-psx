//! Generic native components: the Unity MonoBehaviours every enemy and boss shares and no FSM owns
//! (docs/FSM-V2-DESIGN.md 2.4). `HealthManager` is `ActorHealth` (actors.rs, with the hit flash); `Recoil` is the actor
//! runtime's `recoil_step!` (game/src/enemies.rs, a macro because the solver call has to stay inline in each family);
//! the death effects' corpse fling is here.
//!
//! Written from what the original does at the boundary (the order of events, the numbers its
//! components carry), not from its code. A family keeps its own FSM; these are the pieces its
//! adapter calls so that the next family does not write them again.

/// Ticks a landed hit flashes the sprite (`SpriteFlash.flashInfected`).
pub const FLASH_TICKS: u8 = 16;

/// `EnemyDeathEffects`' corpse fling for a boss whose corpse is flung at `speed`: sideways at 60 degrees
/// away from the Knight for a horizontal strike, straight up or down for the others.
pub fn death_fling(kind: u16, away: i32, speed: i32) -> [i32; 2] {
    match kind {
        2 => [0, speed],
        3 => [0, -speed],
        _ => [away * (speed >> 1), psx_math::int32::mul_shr_i32(speed, 56756, 16)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;

    #[test]
    fn fling_matches_the_mawlek_corpse_numbers() {
        let s = 12 * ONE;
        assert_eq!(death_fling(2, 1, s), [0, s]);
        assert_eq!(death_fling(3, 1, s), [0, -s]);
        assert_eq!(death_fling(0, -1, s), [-(s >> 1), psx_math::int32::mul_shr_i32(s, 56756, 16)]);
    }

    #[test]
    fn a_landed_hit_flashes_and_a_blocked_one_does_not() {
        use crate::{ActorHealth, EnemyParams, Hit};
        let p = EnemyParams { health: 10, contact_damage: 1, evasion_ticks: 4, invincible: false, damage_override: false };
        let mut h = ActorHealth::new(p);
        assert_eq!(h.strike(EnemyParams { invincible: true, ..p }, 5), Hit::Blocked);
        assert_eq!(h.flash, 0);
        assert_eq!(h.strike(p, 5), Hit::Damaged);
        assert_eq!(h.flash, FLASH_TICKS);
        h.tick_flash();
        assert_eq!(h.flash, FLASH_TICKS - 1);
        h.refill(p);
        assert_eq!((h.hp, h.evasion_ticks, h.flash), (10, 0, FLASH_TICKS - 1), "SetHP leaves the flash to run out");
    }
}
