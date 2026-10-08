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
    pub const CAP: usize = 416;
}
#[path = "../game/src/charms.rs"]
pub mod charms;
