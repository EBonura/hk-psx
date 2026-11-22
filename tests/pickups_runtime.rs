//! Compile the real chest and pickup runtime against the cooked tables.
//! Presentation is stubbed by `#[cfg(not(test))]`.
#[path = "../game/src/pickups.rs"]
pub mod pickups;

/// The opened-chest tink (game/src/scene_sfx.rs), silent here.
mod scene_sfx {
    pub fn tink() {}
}
