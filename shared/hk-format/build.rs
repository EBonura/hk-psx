//! Sets `cooked_data` when a cook has written the room packs that
//! `tests/occluder_admission.rs` checks, so a clean checkout reports that test as
//! skipped instead of passing it without checking anything.
use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(cooked_data)");
    let regions = Path::new("../../data/regions");
    if regions.exists() {
        println!("cargo::rustc-cfg=cooked_data");
    }
    // Watch the nearest path that exists, so a cook that creates it re-runs this,
    // and a clean checkout does not rebuild every time (cargo treats a missing
    // watched path as always changed).
    let watched = [regions, Path::new("../../data"), Path::new("../..")]
        .into_iter()
        .find(|p| p.exists())
        .unwrap();
    println!("cargo::rerun-if-changed={}", watched.display());
}
