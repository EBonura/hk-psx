//! Compile the real shop module against the cooked stock and the real charm
//! inventory, the way `tests/charms_runtime.rs` compiles the charm board.
//!
//! Nothing here is stubbed. A charm purchase writes into the same
//! `charms::State` the pause screen equips from, so the two halves of "obtain a
//! charm" are tested against each other rather than against a mock.
pub mod dialogue {
    pub const CAP: usize = 360;
}
#[path = "../game/src/charms.rs"]
pub mod charms;
#[path = "../game/src/shop.rs"]
pub mod shop;
// A purchase invalidates the cached live parameters, which the cheats read.
include!("../data/params.rs");
#[allow(dead_code, clippy::all, unexpected_cfgs)]
#[path = "../game/src/cheats.rs"]
mod cheats;
#[allow(dead_code)]
#[path = "../game/src/hero_params.rs"]
mod hero_params;
