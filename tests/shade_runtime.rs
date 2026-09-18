//! Compile the real Hollow Shade runtime against the cooked art and hk_sim.
//! Presentation is stubbed; the record, spawn rule and frame table are real.
pub mod disc{pub const SCENE_TEXTURE_CAPACITY:usize=1152;}
pub const KNIGHT_SCALE:i32=0;
#[path="../game/src/shade.rs"]pub mod shade;
