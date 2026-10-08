//! Compile the real shop module against the cooked stock and the real charm
//! inventory, the way `tests/charms_runtime.rs` compiles the charm board.
//!
//! Nothing here is stubbed. A charm purchase writes into the same
//! `charms::State` the pause screen equips from, so the two halves of "obtain a
//! charm" are tested against each other rather than against a mock.
pub mod dialogue {
    pub const CAP: usize = 416;
}
#[path = "../game/src/charms.rs"]
pub mod charms;
#[path = "../game/src/shop.rs"]
pub mod shop;
