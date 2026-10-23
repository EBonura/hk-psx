//! Compile the real charm module against the cooked catalogue and hk_sim.
//! Presentation is stubbed out by `#[cfg(not(test))]`; the notch board, the
//! overcharm rule, the effect composition and the card record are real, and so
//! is the cooked board geometry, which is arithmetic rather than drawing: the
//! icon size that set the row pitch has to leave the description and the footer
//! room inside 240 scanlines whether or not there is a GPU here.
//!
//! `dialogue`'s glyph budget is the one thing the panel needs from outside, and
//! `tests/test_charms.py` holds this copy of it to the value the real module
//! declares so the two cannot drift apart unnoticed.
pub mod dialogue {
    pub const CAP: usize = 360;
}
#[path = "../game/src/charms.rs"]
pub mod charms;

// The live board against the real damage path: `cheats::Settings::hurt` is where every hit on the
// Knight lands, and `HeroController::TakeDamage` hands `TakeHealth` twice the damage while
// `overcharmed`. cheats.rs is compiled for real here (only the shop's half of PlayerData is stubbed).
mod shop {
    pub fn vitals(base: hk_sim::VitalParams) -> hk_sim::VitalParams {
        base
    }
}
#[allow(clippy::all, unexpected_cfgs, dead_code)]
#[path = "../game/src/cheats.rs"]
mod cheats;
#[test]
fn an_overcharmed_hit_takes_two_masks_through_the_real_damage_path() {
    use hk_sim::{Hurt, VitalParams, Vitals};
    let base = VitalParams {
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
    let hit = |expected_health: u16| {
        let mut v = Vitals::new(base);
        let settings = cheats::Settings::new();
        assert_eq!(settings.hurt(&mut v, base, 1, 1, false, false), Hurt::Recoiling);
        assert_eq!(v.health, expected_health);
    };
    *charms::state() = charms::State::new();
    // Grubsong (1 notch) and Soul Eater (4) on the board's three notches: one notch is open
    // when the second goes on, so it is allowed once the overcharm has been earned.
    let board = charms::state();
    for charm in [3, 21] {
        board.give(charm);
    }
    board.can_overcharm = true;
    assert_eq!(board.toggle(3), Ok(true));
    hit(4);
    assert_eq!(board.toggle(21), Ok(true));
    assert!(board.overcharmed());
    hit(3);
    // Taking one off puts the spend back within the board.
    assert_eq!(board.toggle(21), Ok(false));
    assert!(!board.overcharmed());
    hit(4);
    *charms::state() = charms::State::new();
}
