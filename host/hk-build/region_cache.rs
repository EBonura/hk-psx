//! Per-region receipts for a cook interrupted before final provenance exists.
//! The build driver hashes the entire selected retail source before and after
//! cooking. These receipts bind each completed region to that full input set.

use super::{sha256_file, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const SHARED: &[&str] = &[
    "data/params.rs",
    ".hkpsx/gameplay-source.json",
    ".hkpsx/focus-source.json",
    ".hkpsx/vitals-source.json",
    ".hkpsx/combat-source.json",
    ".hkpsx/provenance.json",
];
const CORE: &[&str] = &[
    "room.hk",
    "scene.json",
    "unsupported.json",
    "tilemap-fill.json",
];

fn region_dir(root: &Path, name: &str) -> Result<PathBuf> {
    // `region-NNN`, and `region-NNNN` once the catalogue passes 999 chunks.
    if !(10..=11).contains(&name.len())
        || !name.starts_with("region-")
        || !name[7..].bytes().all(|b| b.is_ascii_digit())
        || name == "region-000"
    {
        return Err("invalid region cache name".into());
    }
    Ok(root.join("data/regions").join(name))
}

fn source_manifest(root: &Path) -> Result<(Value, String)> {
    let path = root.join(".hkpsx/regions-source-inputs.json");
    let digest = sha256_file(&path)?;
    if std::env::var("HK_SOURCE_FINGERPRINT_CHECKED")
        .ok()
        .as_deref()
        != Some(&digest)
    {
        return Err("region source manifest was not checked by the build driver".into());
    }
    let manifest: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    Ok((manifest, digest))
}

fn full_inputs_digest(inputs: &serde_json::Map<String, Value>) -> Result<String> {
    let mut hasher = Sha256::new();
    for (name, hash) in inputs {
        let hash = hash.as_str().ok_or("invalid retail input hash")?;
        hasher.update(format!("{name}:{hash}\n"));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn valid_key(key: &Value, manifest: &Value) -> Result<()> {
    if key["interim"] != true || key["source"] != manifest["source"] {
        return Err("region cache source does not match selected retail root".into());
    }
    let inputs = manifest["inputs"]
        .as_object()
        .ok_or("source manifest has no inputs")?;
    if inputs.is_empty() || key["shas"] != manifest["inputs"] {
        return Err("region cache input set differs from full retail manifest".into());
    }
    let names: Vec<_> = inputs
        .keys()
        .map(|name| Value::String(name.clone()))
        .collect();
    if key["names"] != Value::Array(names)
        || key["inputs"] != full_inputs_digest(inputs)?
        || key["key"].as_str().is_none_or(str::is_empty)
    {
        return Err("region cache key does not bind the full retail input set".into());
    }
    Ok(())
}

fn output_path(root: &Path, region: &Path, name: &str) -> Option<PathBuf> {
    if let Some(local) = name.strip_prefix("file:") {
        if local.is_empty()
            || local == "cook-cache.json"
            || local.contains('/')
            || local.contains('\\')
            || local == "."
            || local == ".."
        {
            return None;
        }
        return Some(region.join(local));
    }
    let shared = name.strip_prefix("shared:")?;
    if SHARED.contains(&shared) && region.file_name()?.to_str()? == "region-001" {
        Some(root.join(shared))
    } else {
        None
    }
}

fn expected_outputs(
    root: &Path,
    region: &Path,
    cooked: &Value,
) -> Result<BTreeMap<String, String>> {
    let mut outputs = BTreeMap::new();
    for entry in std::fs::read_dir(region)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "cook-cache.json" || name.ends_with(".tmp") {
            continue;
        }
        if !entry.file_type()?.is_file()
            || output_path(root, region, &format!("file:{name}")).is_none()
        {
            return Err("unsafe region output entry".into());
        }
        outputs.insert(format!("file:{name}"), sha256_file(&path)?);
    }
    for name in CORE {
        if !outputs.contains_key(&format!("file:{name}")) {
            return Err(format!("required region output missing: {name}").into());
        }
    }
    if region.file_name().and_then(|n| n.to_str()) == Some("region-001") {
        outputs.insert(
            "file:palettes.bin".into(),
            sha256_file(&region.join("palettes.bin"))?,
        );
        for page in 0..cooked["pages"]
            .as_u64()
            .ok_or("region page count missing")?
        {
            let name = format!("page-{page}.bin");
            outputs.insert(format!("file:{name}"), sha256_file(&region.join(name))?);
        }
        for name in SHARED {
            outputs.insert(format!("shared:{name}"), sha256_file(&root.join(name))?);
        }
    }
    if outputs["file:room.hk"]
        != cooked["pack_sha256"]
            .as_str()
            .ok_or("cooked pack hash missing")?
    {
        return Err("cooked room hash differs from pack report".into());
    }
    Ok(outputs)
}

fn record(
    root: &Path,
    name: &str,
    key: &Value,
    cooked: &Value,
    manifest: &Value,
    manifest_sha: &str,
) -> Result<()> {
    valid_key(key, manifest)?;
    let region = region_dir(root, name)?;
    let outputs = expected_outputs(root, &region, cooked)?;
    let receipt = json!({"schema": 2, "key": key["key"], "source": key["source"],
        "inputs": key["inputs"], "source_manifest_sha256": manifest_sha,
        "outputs": outputs, "cooked": cooked});
    let final_path = region.join("cook-cache.json");
    let temp = region.join(format!("cook-cache.{}.tmp", std::process::id()));
    std::fs::write(&temp, serde_json::to_vec(&receipt)?)?;
    std::fs::rename(temp, final_path)?;
    Ok(())
}

fn check(
    root: &Path,
    name: &str,
    key: &Value,
    manifest: &Value,
    manifest_sha: &str,
) -> Result<Option<Value>> {
    valid_key(key, manifest)?;
    let region = region_dir(root, name)?;
    let bytes = match std::fs::read(region.join("cook-cache.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let old: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if old["schema"] != 2
        || old["key"] != key["key"]
        || old["source"] != key["source"]
        || old["inputs"] != key["inputs"]
        || old["source_manifest_sha256"] != manifest_sha
    {
        return Ok(None);
    }
    let Some(saved) = old["outputs"].as_object() else {
        return Ok(None);
    };
    let Some(cooked) = old.get("cooked") else {
        return Ok(None);
    };
    let expected = match expected_outputs(root, &region, cooked) {
        Ok(outputs) => outputs,
        Err(_) => return Ok(None),
    };
    if saved.len() != expected.len() {
        return Ok(None);
    }
    let required: BTreeSet<_> = expected.keys().collect();
    if required != saved.keys().collect() {
        return Ok(None);
    }
    if expected.iter().any(|(name, hash)| saved[name] != *hash) {
        return Ok(None);
    }
    Ok(Some(cooked.clone()))
}

pub fn run(root: &Path, action: &str, name: &str) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    let key = &request["key"];
    let (manifest, sha) = source_manifest(root)?;
    match action {
        "region-cache-check" => println!(
            "{}",
            serde_json::to_string(&check(root, name, key, &manifest, &sha)?)?
        ),
        "region-cache-record" => record(root, name, key, &request["cooked"], &manifest, &sha)?,
        _ => return Err("invalid region cache action".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, Value, Value, Value) {
        let root =
            std::env::temp_dir().join(format!("hk-region-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let region = root.join("data/regions/region-002");
        std::fs::create_dir_all(&region).unwrap();
        for file in ["scene.json", "unsupported.json", "tilemap-fill.json"] {
            std::fs::write(region.join(file), b"{}").unwrap();
        }
        std::fs::write(region.join("room.hk"), b"room").unwrap();
        let manifest = json!({"source":"/retail/one", "inputs":{"a":"hash-a"}});
        let inputs = manifest["inputs"].as_object().unwrap();
        let key = json!({"key":"region-code-key", "source":"/retail/one", "interim":true,
            "names":["a"], "shas":manifest["inputs"], "inputs":full_inputs_digest(inputs).unwrap()});
        let cooked =
            json!({"pack_sha256":sha256_file(&region.join("room.hk")).unwrap(), "pages":0});
        (root, manifest, key, cooked)
    }

    #[test]
    fn interrupted_receipt_rejects_output_tampering_and_missing_required_files() {
        let (root, manifest, key, cooked) = fixture("outputs");
        record(&root, "region-002", &key, &cooked, &manifest, "manifest").unwrap();
        assert_eq!(
            check(&root, "region-002", &key, &manifest, "manifest").unwrap(),
            Some(cooked)
        );
        let scene = root.join("data/regions/region-002/scene.json");
        std::fs::write(&scene, b"changed").unwrap();
        assert!(check(&root, "region-002", &key, &manifest, "manifest")
            .unwrap()
            .is_none());
        std::fs::remove_file(&scene).unwrap();
        assert!(record(
            &root,
            "region-002",
            &key,
            &json!({"pack_sha256":"x"}),
            &manifest,
            "manifest"
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_receipt_binds_full_input_set_and_source_root() {
        let (root, manifest, key, cooked) = fixture("inputs");
        record(&root, "region-002", &key, &cooked, &manifest, "manifest").unwrap();
        let added = json!({"source":"/retail/one", "inputs":{"a":"hash-a", "b":"hash-b"}});
        assert!(check(&root, "region-002", &key, &added, "manifest").is_err());
        let changed_root = json!({"source":"/retail/two", "inputs":{"a":"hash-a"}});
        assert!(check(&root, "region-002", &key, &changed_root, "manifest").is_err());
        assert!(
            check(&root, "region-002", &key, &manifest, "other-manifest")
                .unwrap()
                .is_none()
        );
        assert!(region_dir(&root, "region-../").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn first_region_requires_shared_metadata() {
        let (root, manifest, key, cooked) = fixture("shared");
        std::fs::rename(
            root.join("data/regions/region-002"),
            root.join("data/regions/region-001"),
        )
        .unwrap();
        assert!(record(&root, "region-001", &key, &cooked, &manifest, "manifest").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
