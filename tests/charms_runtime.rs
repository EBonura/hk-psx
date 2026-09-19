//! Compile the real charm module against the cooked catalogue and hk_sim.
//! Presentation is stubbed out by `#[cfg(not(test))]`; the notch board, the
//! overcharm rule, the effect composition and the card record are real.
//!
//! `dialogue`'s glyph budget is the one thing the panel needs from outside, and
//! `tests/test_charms.py` holds this copy of it to the value the real module
//! declares so the two cannot drift apart unnoticed.
pub mod dialogue {
    pub const CAP: usize = 416;
}
#[path = "../game/src/charms.rs"]
pub mod charms;
