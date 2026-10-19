//! The Knight's look clips against the cooked ability art: the state machine in
//! game/src/look_pose.rs and the loop-section frame choice in game/src/ability_art.rs.
pub mod disc{pub const SCENE_TEXTURE_CAPACITY:usize=1152;}
pub const KNIGHT_SCALE:i32=0;
#[path="../game/src/shade.rs"]pub mod shade;
#[path="../game/src/ability_art.rs"]pub mod ability_art;
#[path="../game/src/look_pose.rs"]pub mod look_pose;
