//! Native run of the Grubfather's `King Control` (`game/src/grubfather.rs`)
//! against the cooked reward table. The guest half (the one King, the payout and
//! its telemetry) is out of the test build; the state machine it drives and the
//! coin draw are all here.
#![allow(dead_code)]
/// The grants the reward table names, as game/src/pickups.rs declares them
/// (pickups.rs itself needs the hk_sim crate).
pub mod pickups {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Grant {
        Charm(u8),
        Trinket(u8),
        CityKey,
        MaskShard,
        VesselFragment,
        RancidEgg,
        Ability(u8),
    }
}
#[path = "../game/src/grubfather.rs"]
mod grubfather;
