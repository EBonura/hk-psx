//! hk-psx's cookers in Rust, replacing host/*.py one tool at a time with
//! byte-identical output.

pub mod climber;
pub mod colliders;
pub mod common;
pub mod cook_audio;
pub mod actor_specs;
pub mod actors;
pub mod ambience;
pub mod area_music;
pub mod aspid;
pub mod baldur;
pub mod break_effects;
pub mod cook;
pub mod coverage;
pub mod false_knight;
pub mod fmod;
pub mod focus_audio;
pub mod music;
pub mod music_report;
pub mod geo_audio;
pub mod gpu_census;
pub mod gruzzer;
pub mod hatcher;
pub mod husk_guard;
pub mod materials;
pub mod mawlek;
pub mod opaque_groups;
pub mod opaque_tiles;
pub mod blocker;
pub mod pigeon;
pub mod png;
pub mod region_delta;
pub mod props;
pub mod pyfloat;
pub mod recog;
pub mod pyjson;
pub mod runner;
pub mod runner_audio;
pub mod scene_certificates;
pub mod scene_sfx;
pub mod spu;
pub mod vengefly;
pub mod zombie_shield;
pub mod vitals;
pub mod xa_music;

/// Recorded where a Python cooker recorded the sha256 of its own source:
/// the sha256 of the Rust module that replaced it (host/props.py -> props.rs).
pub fn tool_sha256() -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(include_bytes!("props.rs")).iter().map(|b| format!("{b:02x}")).collect()
}
