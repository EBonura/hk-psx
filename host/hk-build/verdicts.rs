//! A route's validate verdict, kept so that a validate that is killed, repeated on an identical disc
//! or resumed after a lane fixed one route does not replay what already passed.
//!
//! The key names everything the verdict depends on: the disc, the replay frontend, the judge (this
//! executable and the two Python helpers it calls), the route's tape, pin file and card fixture.
//! A verdict is only ever read with `validate --resume`, and only for open-loop routes with no cut
//! list (the journey segments chain cards and the closed-loop routes are judged by the gate).
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The verdict of one passed route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// The route passed under the documented baseline input gap (counted apart from plain passes).
    pub baseline_qualified: bool,
    /// The line validate printed for it.
    pub line: String,
    /// Its entry in `summary.json`.
    pub summary: Value,
}

fn dir(root: &Path, name: &str) -> PathBuf {
    root.join(".hkpsx/validate-verdicts").join(name)
}

/// One digest over `parts` (length-prefixed, so no two part lists collide).
pub fn key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The stored verdict of `name`, if it was made under `key`.
pub fn load(root: &Path, name: &str, key: &str) -> Option<Entry> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(dir(root, name).join("verdict.json")).ok()?).ok()?;
    (value["key"] == key && value["pass"] == true).then(|| Entry {
        key: key.to_string(),
        baseline_qualified: value["baseline_qualified"] == true,
        line: value["line"].as_str().unwrap_or_default().to_string(),
        summary: value["summary"].clone(),
    })
}

/// Keep a passed route's verdict, with the replay record (`command.json`, `replay.log`) the later
/// checks read, in a directory of its own that a new validate run does not wipe.
pub fn store(root: &Path, name: &str, entry: &Entry, output: &Path) -> Result<()> {
    let target = dir(root, name);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(&target)?;
    for file in ["command.json", "replay.log"] {
        if output.join(file).is_file() {
            std::fs::copy(output.join(file), target.join(file))?;
        }
    }
    let value = json!({
        "key": entry.key, "pass": true, "baseline_qualified": entry.baseline_qualified,
        "line": entry.line, "summary": entry.summary,
    });
    // Written last: a verdict without its record is never read.
    let temp = target.join("verdict.json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&value)?)?;
    Ok(std::fs::rename(temp, target.join("verdict.json"))?)
}

/// Put a cached route's replay record back where this run's later checks look for it.
pub fn restore_record(root: &Path, name: &str, output: &Path) -> Result<()> {
    std::fs::create_dir_all(output)?;
    for file in ["command.json", "replay.log"] {
        let from = dir(root, name).join(file);
        if from.is_file() {
            std::fs::copy(from, output.join(file))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hk-verdicts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_key_follows_every_part_and_cannot_be_confused_by_splitting() {
        assert_eq!(key(&["a", "b"]), key(&["a", "b"]));
        assert_ne!(key(&["a", "b"]), key(&["a", "c"]));
        assert_ne!(key(&["ab", "c"]), key(&["a", "bc"]));
        assert_ne!(key(&["a"]), key(&["a", ""]));
    }

    #[test]
    fn a_stored_verdict_is_read_back_only_under_its_key_and_brings_its_record() {
        let root = scratch("root");
        let output = root.join("out/route");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("command.json"), "{\"final_ram\": {}}").unwrap();
        std::fs::write(output.join("dump.bin"), vec![0u8; 100]).unwrap();
        let entry = Entry {
            key: key(&["disc"]),
            baseline_qualified: true,
            line: "PASS route: completed=true".into(),
            summary: json!({"pass": false, "baseline_qualified": true}),
        };
        store(&root, "route", &entry, &output).unwrap();
        assert_eq!(load(&root, "route", &key(&["disc"])), Some(entry.clone()));
        assert_eq!(load(&root, "route", &key(&["other disc"])), None);
        assert_eq!(load(&root, "missing", &key(&["disc"])), None);
        // The record comes back, the dump (scratch) does not.
        let again = root.join("out2/route");
        restore_record(&root, "route", &again).unwrap();
        assert!(again.join("command.json").is_file() && !again.join("dump.bin").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_verdict_without_its_final_rename_is_not_read() {
        let root = scratch("torn");
        std::fs::create_dir_all(dir(&root, "route")).unwrap();
        std::fs::write(dir(&root, "route").join("verdict.json.tmp"), "{}").unwrap();
        assert_eq!(load(&root, "route", "k"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
