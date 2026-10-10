//! Sets `cooked_data` when a cook has written the generated game data that the
//! ambience tests compile against, so a clean checkout builds and skips them.
use std::path::Path;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(cooked_data)");
    let file = Path::new("../../data/ambience.rs");
    if file.exists() {
        println!("cargo::rustc-cfg=cooked_data");
    }
    // Watch the nearest path that exists, so a cook that creates the file (or
    // its directory) re-runs this, and a clean checkout does not rebuild every
    // time (cargo treats a missing watched path as always changed).
    let watched = [file, Path::new("../../data"), Path::new("../..")]
        .into_iter()
        .find(|p| p.exists())
        .unwrap();
    println!("cargo::rerun-if-changed={}", watched.display());
}
