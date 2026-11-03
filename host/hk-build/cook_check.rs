//! `cargo hk-build cook-check`: does the incremental cook equal a full recook?
//!
//! The cook caches (receipts, the stage record, the cook store) are only worth having while a cached build is
//! byte for byte what a full cook would have made. This clones the checkout (APFS clone, no disk until
//! written), removes every receipt and cooked region in the clone, cooks it again with every cache ignored
//! (`--recook`, which sets HK_FORCE_COOK) and compares `data/` and `.hkpsx/` of the clone with this checkout.
//! Any difference is printed and the command fails.
//!
//! The clone runs this same binary with `HK_BUILD_ROOT` pointing at it, so nothing is rebuilt. It is a full
//! recook: take a cook slot first (lane-infra/cookslot.sh).

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Paths (relative to the checkout) that are bookkeeping of the cache itself, lane state or scratch, so
/// they are not part of what a cook produces. A prefix ends with `/`.
const NOT_COMPARED: &[&str] = &[
    ".hkpsx/cookdeps/",
    ".hkpsx/stagecache/",
    ".hkpsx/precook-logs/",
    ".hkpsx/validate/",
    ".hkpsx/pgo/",
    ".hkpsx/emulator/",
    ".hkpsx/doctor.json",
    ".hkpsx/sdk.json",
    ".hkpsx/cook-store.json",
    ".hkpsx/python-lock.sha256",
    // Written by every region cook, last one wins; only its input list is read (docs/COOK-CACHE.md).
    ".hkpsx/provenance.json",
    ".hkpsx/regions-provenance.json",
    // Cache verdicts name the checkout they were written in and hold timestamps or the other caches' keys.
    ".hkpsx/regions-cook-cache.json",
    ".hkpsx/assets-cache.json",
    ".hkpsx/menu-cache.json",
    ".hkpsx/typetree-cache.json",
    ".hkpsx/typetree-cache.json.lock",
];

/// Names inside a region directory that are receipts, not cooked output.
const RECEIPTS: &[&str] = &["scenery-cache.json", "annotations-cache.json", "cook-cache.json"];

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn compared(relative: &str) -> bool {
    if relative.starts_with("data/regions/region-") {
        return !RECEIPTS.iter().any(|name| relative.ends_with(&format!("/{name}")));
    }
    !NOT_COMPARED.iter().any(|skip| relative.starts_with(skip))
}

fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let relative = path.strip_prefix(base)?.to_string_lossy().to_string();
        if kind.is_dir() {
            walk(base, &path, out)?;
        } else if kind.is_file() && compared(&relative) {
            out.insert(relative, path);
        }
    }
    Ok(())
}

fn tree(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    for top in ["data", ".hkpsx"] {
        if root.join(top).is_dir() {
            walk(root, &root.join(top), &mut files)?;
        }
    }
    Ok(files)
}

/// Compare two checkouts' cooked state; the differences, in path order.
pub fn differences(left: &Path, right: &Path) -> Result<Vec<String>> {
    let (a, b) = (tree(left)?, tree(right)?);
    let mut out = Vec::new();
    for (name, path) in &a {
        match b.get(name) {
            None => out.push(format!("only in the incremental tree: {name}")),
            Some(other) => {
                if sha256_file(path)? != sha256_file(other)? {
                    out.push(format!("differs: {name}"));
                }
            }
        }
    }
    for name in b.keys() {
        if !a.contains_key(name) {
            out.push(format!("only in the full recook: {name}"));
        }
    }
    Ok(out)
}

pub fn run(root: &Path, keep: bool) -> Result<()> {
    let name = root.file_name().ok_or("root has no name")?.to_string_lossy().to_string();
    let clone = root.with_file_name(format!("{name}-cookcheck"));
    if clone.exists() {
        std::fs::remove_dir_all(&clone)?;
    }
    println!("cook-check: cloning {} to {}", root.display(), clone.display());
    let status = Command::new("cp").args(["-cR"]).arg(root).arg(&clone).status()?;
    if !status.success() {
        return Err("cloning the checkout failed".into());
    }
    // A full recook starts from no cooked regions and no receipts of any kind.
    for stale in ["data/regions", ".hkpsx/cookdeps", ".hkpsx/stagecache", ".hkpsx/regions-cook-cache.json",
                  ".hkpsx/alpha-covers-cache.json"] {
        let path = clone.join(stale);
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    let exe = std::env::current_exe()?;
    println!("cook-check: full recook in the clone");
    let status = Command::new(exe)
        .args(["cook", "--recook"])
        .env("HK_BUILD_ROOT", &clone)
        .current_dir(&clone)
        .status()?;
    if !status.success() {
        return Err(format!("the full recook failed with {status}").into());
    }
    let found = differences(root, &clone)?;
    if found.is_empty() {
        println!("cook-check: data/ and .hkpsx/ are byte-identical to a full recook");
    } else {
        for line in found.iter().take(50) {
            println!("cook-check: {line}");
        }
        println!("cook-check: {} difference(s)", found.len());
    }
    if !keep {
        std::fs::remove_dir_all(&clone)?;
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err("the incremental cook differs from a full recook".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hk-cook-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, relative: &str, bytes: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn receipts_and_bookkeeping_are_not_compared_but_cooked_files_are() {
        let (a, b) = (scratch("a"), scratch("b"));
        for (root, receipt) in [(&a, "one"), (&b, "two")] {
            put(root, "data/regions/region-001/room.hk", "pack");
            put(root, "data/regions/region-001/scenery-cache.json", receipt);
            put(root, ".hkpsx/cookdeps/x.json", receipt);
            put(root, ".hkpsx/provenance.json", receipt);
        }
        assert!(differences(&a, &b).unwrap().is_empty());
        put(&b, "data/regions/region-001/room.hk", "other");
        put(&b, "data/extra.rs", "x");
        put(&a, "data/regions/region-002/scene.json", "{}");
        assert_eq!(
            differences(&a, &b).unwrap(),
            vec![
                "differs: data/regions/region-001/room.hk".to_string(),
                "only in the incremental tree: data/regions/region-002/scene.json".to_string(),
                "only in the full recook: data/extra.rs".to_string(),
            ]
        );
        std::fs::remove_dir_all(&a).unwrap();
        std::fs::remove_dir_all(&b).unwrap();
    }
}
