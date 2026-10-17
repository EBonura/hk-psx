//! Whole-game audio census of the Windows Hollow Knight install.
//!
//!   hk-audio-census extract <hollow_knight_Data> <out dir>
//!
//! Reads every AudioClip header (name, channels, rate, length, stored size;
//! never the sample data), every MusicCue and AtmosCue, and for each of the
//! 501 BuildSettings scenes the clips its objects can play: a PPtr to an
//! AudioClip anywhere in the scene, or reachable through the prefabs and
//! ScriptableObjects the scene points at. Writes tab separated tables of
//! names, counts and durations and a markdown summary. No audio is read or
//! written, so the output is safe to commit.

mod model;
mod scan;
mod summary;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 || args[1] != "extract" {
        eprintln!("usage: hk-audio-census extract <hollow_knight_Data> <out dir>");
        std::process::exit(2);
    }
    if let Err(e) = scan::run(&args[2], std::path::Path::new(&args[3])) {
        eprintln!("hk-audio-census: {e}");
        std::process::exit(1);
    }
}
