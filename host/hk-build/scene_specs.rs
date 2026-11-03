//! The scene spec chunks: every scene's `ActorSpec`s and `CorpseSpec`s leave the resident image and
//! travel with the scene's disc group (docs/SCENE-SPECS.md).
//!
//! The cook writes the specs as Rust text in `data/regions.rs` (`static SCENE_ACTORS_n`), and three
//! post-passes patch that text after the guest prepass. This step runs after all of them, on every
//! build: it cuts the spec statics out of regions.rs into `.hkpsx/specpack/scene_actors.rs` (rustc
//! is the parser: host/hk-specpack compiles that file in against hk-sim and encodes with
//! hk-sim's wire format), writes `data/regions_guest.rs` (regions.rs without them, which the guest
//! includes), the chunks (`data/scene-specs/scene_<id>.hkspec`) and `data/scene_specs.rs`.
//!
//! Nothing may change a spec afterwards: `data/scene_specs.rs` records the FNV-1a of the spec text
//! the chunks were packed from, and `verify` (run by the disc packer) refuses a disc whose
//! regions.rs specs no longer hash to it.
use super::Result;
use std::path::Path;
use std::process::Command;

const SPEC_FIRST: &str = "static SCENE_ACTORS_0:";
const SPEC_TABLE: &str = "pub static SCENE_ACTORS:";

/// FNV-1a over `bytes`, the hash the generated constants carry.
pub fn fnv(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for &b in bytes {
        hash = (hash ^ u32::from(b)).wrapping_mul(0x01000193);
    }
    hash
}

/// The spec statics of `regions` (from `static SCENE_ACTORS_0:` through the `SCENE_ACTORS` table line)
/// and the rest of the file. Every item that starts a line in between must be a `SCENE_ACTORS_n`
/// static, so a new generated table can never be cut out by accident.
pub fn split(regions: &str) -> Result<(String, String)> {
    let lines: Vec<&str> = regions.split_inclusive('\n').collect();
    let first = lines
        .iter()
        .position(|l| l.starts_with(SPEC_FIRST))
        .ok_or("regions.rs has no SCENE_ACTORS_0 spec table")?;
    let last = lines
        .iter()
        .position(|l| l.starts_with(SPEC_TABLE))
        .ok_or("regions.rs has no SCENE_ACTORS table")?;
    if last < first {
        return Err("regions.rs: the SCENE_ACTORS table precedes its specs".into());
    }
    for line in &lines[first..last] {
        let item = line.starts_with("static ") || line.starts_with("pub ") || line.starts_with("const ");
        if item && !line.starts_with("static SCENE_ACTORS_") {
            return Err(format!("regions.rs: {:?} sits inside the spec block", &line[..line.len().min(60)]).into());
        }
    }
    let specs: String = lines[first..=last].concat();
    let mut rest: String = lines[..first].concat();
    rest.push_str("// The spec statics left data/regions.rs for the scene spec chunks (host/hk-build/scene_specs.rs).\n");
    rest.push_str(&lines[last + 1..].concat());
    Ok((specs, rest))
}

fn read_recorded_fnv(root: &Path) -> Result<u32> {
    let text = std::fs::read_to_string(root.join("data/scene_specs.rs"))?;
    text.lines()
        .find_map(|l| l.strip_prefix("pub const SPEC_SOURCE_FNV: u32 = "))
        .and_then(|v| v.trim_end_matches(';').parse().ok())
        .ok_or_else(|| "data/scene_specs.rs carries no SPEC_SOURCE_FNV".into())
}

/// Cut the specs out of regions.rs and pack them. Idempotent: a rerun on unchanged text writes the
/// same bytes.
pub fn pack(root: &Path) -> Result<()> {
    let regions = std::fs::read_to_string(root.join("data/regions.rs"))?;
    let (specs, rest) = split(&regions)?;
    let source = fnv(specs.as_bytes());
    let dir = root.join(".hkpsx/specpack");
    std::fs::create_dir_all(&dir)?;
    publish_text(&dir.join("scene_actors.rs"), &specs)?;
    publish_text(&root.join("data/regions_guest.rs"), &rest)?;
    println!("+ hk-specpack");
    let status = Command::new("cargo")
        .args(["run", "--release", "--quiet", "--manifest-path", "host/hk-specpack/Cargo.toml", "--"])
        .arg(root)
        .arg(source.to_string())
        .env("HK_SPECPACK_SRC", &dir)
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(format!("hk-specpack failed with {status}").into());
    }
    Ok(())
}

/// The equivalence test of host/hk-specpack: every chunk decodes to the generated specs.
pub fn test(root: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .args(["test", "--release", "--quiet", "--manifest-path", "host/hk-specpack/Cargo.toml"])
        .env("HK_SPECPACK_SRC", root.join(".hkpsx/specpack"))
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(format!("the scene spec equivalence test failed with {status}").into());
    }
    Ok(())
}

/// Write `text` unless the file already holds it, so an unchanged file keeps its mtime and the
/// guest crate is not rebuilt for nothing.
fn publish_text(path: &Path, text: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Refuse a build whose regions.rs specs are not the ones the chunks were packed from, or whose
/// guest regions file is not today's regions.rs without them.
pub fn verify(root: &Path) -> Result<()> {
    let regions = std::fs::read_to_string(root.join("data/regions.rs"))?;
    let (specs, rest) = split(&regions)?;
    if read_recorded_fnv(root)? != fnv(specs.as_bytes()) {
        return Err("the ActorSpec text of data/regions.rs changed after the scene spec chunks were packed (a bind ran after host/hk-specpack): rerun the build".into());
    }
    if std::fs::read_to_string(root.join("data/regions_guest.rs"))? != rest {
        return Err("data/regions_guest.rs is not data/regions.rs without its spec statics: rerun the build".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "pub const SCENES: usize = 2;\nstatic PARTICLE_BANK_0: u8 = 0;\nstatic SCENE_ACTORS_0: &[hk_sim::ActorSpec] = &[];\nstatic SCENE_ACTORS_1: &[hk_sim::ActorSpec] = &[hk_sim::ActorSpec {a:1},\nhk_sim::ActorSpec {a:2}];\npub static SCENE_ACTORS: &[&[hk_sim::ActorSpec]] = &[SCENE_ACTORS_0,SCENE_ACTORS_1];\npub static SCENE_EMITTERS: u8 = 0;\n";

    #[test]
    fn the_spec_block_is_cut_and_the_rest_kept() {
        let (specs, rest) = split(SAMPLE).unwrap();
        assert!(specs.starts_with("static SCENE_ACTORS_0:") && specs.ends_with("SCENE_ACTORS_1];\n"));
        assert!(rest.contains("PARTICLE_BANK_0") && rest.contains("SCENE_EMITTERS"));
        assert!(!rest.contains("SCENE_ACTORS"));
    }

    #[test]
    fn a_foreign_item_inside_the_block_is_refused() {
        let text = SAMPLE.replace("static SCENE_ACTORS_1", "static OTHER_TABLE: u8 = 1;\nstatic SCENE_ACTORS_1");
        assert!(split(&text).is_err());
    }

    #[test]
    fn a_missing_table_is_refused() {
        assert!(split("pub const SCENES: usize = 0;\n").is_err());
    }
}
