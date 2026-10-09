//! hk-psx's cookers in Rust, replacing host/*.py one tool at a time with
//! byte-identical output.

pub mod climber;
pub mod colliders;
pub mod common;
pub mod cook_audio;
pub mod actor_art;
pub mod actor_specs;
pub mod alpha_cover;
pub mod atlas;
pub mod packer;
pub mod actors;
pub mod ambience;
pub mod area_music;
pub mod aspid;
pub mod baldur;
pub mod blas;
pub mod break_effects;
pub mod cook;
pub mod coverage;
pub mod effects_art;
pub mod false_knight;
pub mod false_knight_art;
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
pub mod numpy_math;
pub mod numpy_rng;
pub mod opaque_groups;
pub mod opaque_tiles;
pub mod blocker;
pub mod pigeon;
pub mod prefab;
pub mod png;
pub mod polygons;
pub mod quantize;
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
pub mod static_sources;
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
