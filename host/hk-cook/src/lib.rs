//! hk-psx's cookers in Rust, replacing host/*.py one tool at a time with
//! byte-identical output.

pub mod common;
pub mod cook_audio;
pub mod ambience;
pub mod area_music;
pub mod break_effects;
pub mod cook;
pub mod coverage;
pub mod fmod;
pub mod focus_audio;
pub mod music;
pub mod music_report;
pub mod geo_audio;
pub mod gpu_census;
pub mod materials;
pub mod opaque_groups;
pub mod opaque_tiles;
pub mod png;
pub mod region_delta;
pub mod props;
pub mod pyfloat;
pub mod pyjson;
pub mod runner;
pub mod runner_audio;
pub mod scene_certificates;
pub mod scene_sfx;
pub mod spu;
pub mod xa_music;

/// Recorded where a Python cooker recorded the sha256 of its own source:
/// the sha256 of the Rust module that replaced it (host/props.py -> props.rs).
pub fn tool_sha256() -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(include_bytes!("props.rs")).iter().map(|b| format!("{b:02x}")).collect()
}
