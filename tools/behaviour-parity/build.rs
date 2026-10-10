//! Lifts the per-scene `ActorSpec` catalogue out of the cooked `data/regions.rs`.
//!
//! The catalogue is plain `hk_sim::...` literals, one `static SCENE_ACTORS_<scene>` per scene, so
//! this tool links the exact types the guest links without reimplementing the cooker's table. The
//! rest of `regions.rs` names guest-only modules and is left alone.
use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = env::var("HKBP_REGIONS_RS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../../data/regions.rs"));
    println!("cargo:rerun-if-env-changed=HKBP_REGIONS_RS");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = fs::read_to_string(&source).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("static SCENE_ACTORS_") || line.starts_with("pub static SCENE_ACTORS:")
        {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(
        out.contains("pub static SCENE_ACTORS:"),
        "no SCENE_ACTORS table in {}",
        source.display()
    );
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("scene_actors.rs"),
        out,
    )
    .unwrap();
}
